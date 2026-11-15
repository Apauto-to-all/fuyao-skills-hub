//! 写入口共用骨架：hub 写命令的同一套前导与尾声。
//! 入口先做不依赖 hub 的入参判定，随后骨架接手：装配判定 → 开仓（预演只读、
//! 实跑持写锁并封存手改现场）→ 算计划 → 落盘 → 机器提交 → 耗时与里程碑日志。
//! 入库、归档、移除、恢复、使用、撤回六个写入口因此只剩「一份计划」与
//! 「一个落盘函数」，锁姿势、预演分叉与日志形状都只写一遍。
//!
//! 开仓有两种姿势，落盘段只有一份：写入口要求装配完整（[`run`]），
//! 装配入口在仓库尚未建出时也要能开工（[`run_assembly`]）。两种姿势都收
//! `dry_run`：预演只算计划、不取写锁；实跑取全局写锁、封存手改现场，随后
//! 交给同一段落盘：按计划落盘 → 机器提交 → 里程碑日志。首建的仓库由落盘段
//! 建出后经 [`Ctx::attach_repo`] 交回骨架。
//!
//! 盘面动作一律经 [`Ctx`] 的原语做（见 `disk` 子模块）：原语在动手那一刻自己
//! 登记撤销，入口没有别的地方可登记，也没有别的地方会漏登记。
//!
//! 机器提交按计划的提交步落：提交清单在落盘之前读出，落盘段只负责执行它。
//! 预演清单与实跑提交因此同出一份计划，命令只说一次自己要提交什么。
//!
//! 落盘失败时盘面怎么处置由 [`OnFailure`] 显式点名，不藏在各入口的分支里：
//! 回滚（撤销本次已登记的盘上动作）或续跑（保留中途盘面，重跑续完）。
//! 锁内顺序不变量（先取锁 → 再读登记簿 → 最后写盘）由 [`Ctx`] 的字段保证：
//! 登记簿写回只能拿到 `token`，机器提交只经骨架，入口碰不到仓库句柄。

use std::path::Path;
use std::time::Instant;

use crate::error::CommonError;
use crate::git;
use crate::link::LinkError;
use crate::lock;
use crate::paths::HubPaths;
use crate::plan::{Plan, PlanStep};

mod disk;
#[cfg(test)]
mod tests;

pub(crate) use disk::Ctx;

/// 落盘失败时的盘面处置：一次操作要么回到开工前的样子，要么留在中途等重跑续完
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnFailure {
    /// 撤销本次已登记的盘上动作：盘上不留没有记录的入口
    Rollback,
    /// 保留中途盘面：已做的部分不回退，删除类动作重跑即可续完
    Resume,
}

/// 一次落盘交回骨架的两样东西：命令报告本体与里程碑数据
pub(crate) struct Applied<Outcome, Milestone> {
    /// 命令的结果载荷
    pub(crate) outcome: Outcome,
    /// 记里程碑日志要用的数据，由入口自己定义形状
    pub(crate) milestones: Milestone,
}

/// 手改快照步骤：仓库有未提交变更时，实跑会在动盘前先提交一次快照。
/// 预演要列出同一步，否则清单会漏掉这次提交
pub(crate) fn snapshot_step(repo: &git::Repository) -> Option<PlanStep> {
    git::is_dirty(repo)
        .ok()
        .filter(|dirty| *dirty)
        .map(|_| PlanStep::commit(git::sync_message()))
}

/// 一次写命令的执行：入参判定 → 装配判定 → 开仓 → 计划 → 落盘 → 提交与里程碑。
/// 入参判定（键形状、源目录身份、目标集合非空）由入口先做：它们不依赖 hub，
/// 六个写入口一齐遵守——坏入参先报入参错误，不落到「hub 未初始化」。
/// 预演只读开仓、只算计划；实跑持锁落盘。
/// 落盘成功后按计划里的提交步逐条机器提交，再拿 `apply` 交回的里程碑数据
/// 记日志；落盘失败按 `failure` 处置盘面后原样上抛，错误只在入口边界记一次
pub(crate) fn run<T, Outcome, Milestone, E>(
    hub_root: &Path,
    dry_run: bool,
    failure: OnFailure,
    plan: impl FnOnce(&HubPaths) -> Result<Plan<T>, E>,
    dry_outcome: impl FnOnce(Plan<T>, Vec<PlanStep>) -> Outcome,
    apply: impl FnOnce(&mut Ctx<'_>, Plan<T>) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
) -> Result<Outcome, E>
where
    E: From<CommonError> + From<std::io::Error> + From<LinkError>,
{
    if dry_run {
        // 预演：装配判定 + 只读开仓，不取写锁——别人正在写也能拿到动作清单
        let repo = git::open_hub(hub_root).map_err(CommonError::from)?;
        let paths = HubPaths::from_root(hub_root);
        let plan = plan(&paths)?;
        // 骨架在计划之前先做的那几步：手改快照，位于全部步骤最前面
        let command = snapshot_step(&repo).into_iter().collect();
        return Ok(dry_outcome(plan, command));
    }
    // 实跑：装配判定、取全局写锁、开仓、封存手改现场由这一处一次做完。
    // 耗时起点在取锁之前：锁上等待也算这次命令的耗时
    let started = Instant::now();
    let (token, repo, paths) = git::open_hub_for_write(hub_root)?;
    let mut ctx = Ctx::new(&token, &paths, failure);
    ctx.attach_repo(repo);
    let plan = plan(&paths)?;
    land(ctx, plan, apply, milestone, started)
}

/// 装配姿势的执行：装配入口在仓库尚未建出、装配尚不完整时也要开工，
/// 故不走写入口的装配判定。预演只算计划、不取写锁——别人正在写也能拿到
/// 动作清单；实跑取锁 → 仓库已在就开出来并封存手改 → 算计划 → 落盘 →
/// 机器提交 → 里程碑，首建的仓库由落盘段经 [`Ctx::attach_repo`] 交回。
/// 装配的每一步都幂等，盘面一律留在中途等重跑续完
pub(crate) fn run_assembly<T, Outcome, Milestone, E>(
    hub_root: &Path,
    dry_run: bool,
    plan: impl FnOnce(&HubPaths) -> Result<Plan<T>, E>,
    dry_outcome: impl FnOnce(Plan<T>, Vec<PlanStep>) -> Outcome,
    apply: impl FnOnce(&mut Ctx<'_>, Plan<T>) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
) -> Result<Outcome, E>
where
    E: From<git::GitError> + From<lock::LockError>,
{
    let paths = HubPaths::from_root(hub_root);
    if dry_run {
        // 预演：不取写锁，别人正在写也能拿到动作清单。仓库已在就只读开出来，
        // 把骨架先做的那几步（手改快照）一并列出——清单与实跑同出一份前导
        let command = if paths.root().join(".git").exists() {
            let repo = git::Repository::open(paths.root()).map_err(git::GitError::from)?;
            snapshot_step(&repo).into_iter().collect()
        } else {
            Vec::new()
        };
        return Ok(dry_outcome(plan(&paths)?, command));
    }
    // 耗时起点在取锁之前：锁上等待也算这次命令的耗时
    let started = Instant::now();
    let token = lock::write_lock(&paths)?;
    let mut ctx = Ctx::new(&token, &paths, OnFailure::Resume);
    // 仓库已在就先封存手改现场；首建的仓库还没有，由落盘段建出后交回
    if paths.root().join(".git").exists() {
        let repo = git::Repository::open(paths.root()).map_err(git::GitError::from)?;
        git::sync_manual(&repo);
        ctx.attach_repo(repo);
    }
    let plan = plan(&paths)?;
    // 装配的盘面由装配自己照计划做：每一步都幂等，失败留中途重跑即续完
    finish(ctx, plan, apply, milestone, started)
}

/// 写入口的落盘段：先按计划把盘面动作做掉，再交给落盘函数做业务部分。
/// 动作失败按 `failure` 处置盘面后原样上抛——已登记的动作逆序撤干净
fn land<T, Outcome, Milestone, E>(
    mut ctx: Ctx<'_>,
    plan: crate::plan::Plan<T>,
    apply: impl FnOnce(&mut Ctx<'_>, crate::plan::Plan<T>) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
    started: Instant,
) -> Result<Outcome, E>
where
    E: From<std::io::Error> + From<LinkError>,
{
    if let Err(e) = ctx.land(plan.steps()) {
        ctx.settle();
        return Err(e);
    }
    finish(ctx, plan, apply, milestone, started)
}

/// 落盘段共用的尾声：落盘函数 → 机器提交 → 里程碑日志。
/// 提交清单先于落盘从计划读出，落盘段只负责执行它
fn finish<T, Outcome, Milestone, E>(
    mut ctx: Ctx<'_>,
    plan: crate::plan::Plan<T>,
    apply: impl FnOnce(&mut Ctx<'_>, crate::plan::Plan<T>) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
    started: Instant,
) -> Result<Outcome, E> {
    // 计划里的提交步：一条消息一个提交，顺序即计划顺序。没有提交步表示本次
    // 没有可提交的变更，`auto_commit` 不必惊动。提交步缺消息是计划构造的缺陷，
    // 不能静默少提交一次，记 ERROR 留痕
    let mut commits: Vec<String> = Vec::new();
    for step in plan.steps() {
        if step.desc.action != crate::plan::ActionKind::Commit {
            continue;
        }
        match &step.desc.target {
            Some(message) => commits.push(message.clone()),
            None => tracing::error!(action = ?step.desc.action, "提交步缺消息"),
        }
    }
    let Applied {
        outcome,
        milestones,
    } = match apply(&mut ctx, plan) {
        Ok(done) => done,
        Err(e) => {
            // 落盘失败：按策略处置盘面，再把原错误交给入口边界
            ctx.settle();
            return Err(e);
        }
    };
    // 有提交步却没有仓库句柄只可能来自首建的仓库没交回——那是落盘段的缺陷，
    // 不能静默吞掉提交，记 ERROR 留痕
    if let Some(repo) = &ctx.repo {
        for message in &commits {
            git::auto_commit(repo, message);
        }
    } else if !commits.is_empty() {
        tracing::error!(
            count = commits.len(),
            cause = "落盘段没有仓库句柄",
            "机器提交失败"
        );
    }
    milestone(milestones, started.elapsed().as_millis() as u64);
    Ok(outcome)
}
