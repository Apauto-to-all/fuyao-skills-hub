//! 写入口共用骨架：hub 写命令的同一套前导与尾声。
//! 入口先做不依赖 hub 的入参判定，随后骨架接手：装配判定 → 开仓（预演只读、
//! 实跑持写锁并封存手改现场）→ 算计划 → 落盘 → 机器提交 → 耗时与里程碑日志。
//! 入库、归档、移除、恢复、使用、撤回六个写入口因此只剩「计划」与「落盘」，
//! 锁姿势、预演分叉与日志形状都只写一遍。
//!
//! 开仓有两种姿势，落盘段只有一份：写入口要求装配完整（[`run`]），
//! 装配入口在仓库尚未建出时也要能开工（[`run_assembly`]）。两种姿势都先取
//! 全局写锁、再封存手改现场，随后交给同一段 [`land`]：按计划落盘 → 机器提交
//! → 里程碑日志。首建的仓库由落盘段建出后经 [`Ctx::attach_repo`] 交回骨架。
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
use crate::lock::{self, WriteToken};
use crate::paths::HubPaths;
use crate::plan::Steps;

/// 落盘失败时的盘面处置：一次操作要么回到开工前的样子，要么留在中途等重跑续完
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnFailure {
    /// 撤销本次已登记的盘上动作：盘上不留没有记录的入口
    Rollback,
    /// 保留中途盘面：已做的部分不回退，删除类动作重跑即可续完
    Resume,
}

/// 一次实跑的落盘现场：写锁、路径族与本次已做动作的撤销清单
pub(crate) struct Ctx<'a> {
    /// 全局写锁凭证：登记簿写回只认它
    pub(crate) token: &'a WriteToken,
    /// hub 路径族
    pub(crate) paths: &'a HubPaths,
    /// 失败时的盘面处置
    failure: OnFailure,
    /// 本次已做盘上动作的撤销清单，按登记的逆序执行
    undo: Vec<Box<dyn FnOnce()>>,
    /// 本入口的仓库句柄：装配入口在建出仓库之前就开工，故由落盘段交回
    repo: Option<git::Repository>,
}

impl<'a> Ctx<'a> {
    /// 落盘现场：锁凭证与路径族已定，仓库句柄由开仓或落盘段提供
    fn new(token: &'a WriteToken, paths: &'a HubPaths, failure: OnFailure) -> Self {
        Self {
            token,
            paths,
            failure,
            undo: Vec::new(),
            repo: None,
        }
    }

    /// 交回仓库句柄：首建的装配入口在落盘段里才把仓库建出来
    pub(crate) fn attach_repo(&mut self, repo: git::Repository) {
        self.repo = Some(repo);
    }

    /// 登记一条撤销动作：落盘失败且策略为回滚时由骨架逆序执行。
    /// 撤销动作自己负责在删不掉时留痕，骨架只管按序调用
    pub(crate) fn on_rollback(&mut self, undo: impl FnOnce() + 'static) {
        self.undo.push(Box::new(undo));
    }

    /// 失败处置：回滚策略下逆序执行全部撤销动作，续跑策略下只丢弃清单。
    /// 清单随现场析构而清空，不留跨次操作的状态
    fn settle(&mut self) {
        if self.failure == OnFailure::Rollback {
            for undo in self.undo.drain(..).rev() {
                undo();
            }
        }
    }
}

/// 一次落盘交回骨架的两样东西：命令报告本体与里程碑数据。
/// 机器提交写在计划的提交步里，落盘段直接从计划取
pub(crate) struct Applied<Outcome, Milestone> {
    /// 命令的结果载荷
    pub(crate) outcome: Outcome,
    /// 记里程碑日志要用的数据，由入口自己定义形状
    pub(crate) milestones: Milestone,
}

/// 手改快照步骤：仓库有未提交变更时，实跑会在动盘前先提交一次快照。
/// 预演要列出同一步，否则清单会漏掉这次提交
pub(crate) fn snapshot_step(repo: &git::Repository) -> Option<crate::plan::PlanStep> {
    git::is_dirty(repo)
        .ok()
        .filter(|dirty| *dirty)
        .map(|_| crate::plan::PlanStep::commit(git::sync_message()))
}

/// 预演交给出口的两样东西：计划本体，与本次将要执行的完整步骤。
/// 步骤 = 骨架前导（手改快照）++ 计划自身的步骤，与实跑提交的先后一致
pub(crate) struct DryPlan<Plan> {
    /// 计划本体：出口从这里读业务数据
    pub(crate) plan: Plan,
    /// 骨架在计划之外先做的那几步，位于全部步骤的最前面
    pub(crate) preamble: Vec<crate::plan::PlanStep>,
    /// 计划自身的步骤
    pub(crate) steps: Vec<crate::plan::PlanStep>,
}

impl<Plan> DryPlan<Plan> {
    /// 全部步骤：前导在前，计划步骤在后
    pub(crate) fn all_steps(&self) -> Vec<crate::plan::PlanStep> {
        let mut all = self.preamble.clone();
        all.extend(self.steps.iter().cloned());
        all
    }
}

/// 一次写命令的执行：入参判定 → 装配判定 → 开仓 → 计划 → 落盘 → 提交与里程碑。
/// 入参判定（键形状、源目录身份、目标集合非空）由入口先做：它们不依赖 hub，
/// 六个写入口一齐遵守——坏入参先报入参错误，不落到「hub 未初始化」。
/// 预演只读开仓、只算计划；实跑持锁落盘。
/// 落盘成功后按计划里的提交步逐条机器提交，再拿 `apply` 交回的里程碑数据
/// 记日志；落盘失败按 `failure` 处置盘面后原样上抛，错误只在入口边界记一次
pub(crate) fn run<Plan, Outcome, Milestone, E>(
    hub_root: &Path,
    dry_run: bool,
    failure: OnFailure,
    plan: impl FnOnce(&HubPaths) -> Result<Plan, E>,
    dry_outcome: impl FnOnce(DryPlan<Plan>) -> Outcome,
    apply: impl FnOnce(&mut Ctx<'_>, Plan) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
) -> Result<Outcome, E>
where
    Plan: Steps,
    E: From<CommonError>,
{
    if dry_run {
        // 预演：装配判定 + 只读开仓，不取写锁——别人正在写也能拿到动作清单
        let repo = git::open_hub(hub_root).map_err(CommonError::from)?;
        let paths = HubPaths::from_root(hub_root);
        let plan = plan(&paths)?;
        let dry = DryPlan {
            preamble: snapshot_step(&repo).into_iter().collect(),
            steps: plan.steps(),
            plan,
        };
        return Ok(dry_outcome(dry));
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
/// 故不走写入口的装配判定。取锁 → 仓库已在就开出来并封存手改 → 算计划 →
/// 落盘 → 机器提交 → 里程碑；首建的仓库由落盘段经 [`Ctx::attach_repo`] 交回。
/// 盘面一律留在中途等重跑续完：装配的每一步都幂等，重跑即续完
pub(crate) fn run_assembly<Plan, Outcome, Milestone, E>(
    hub_root: &Path,
    plan: impl FnOnce(&HubPaths) -> Result<Plan, E>,
    apply: impl FnOnce(&mut Ctx<'_>, Plan) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
) -> Result<Outcome, E>
where
    Plan: Steps,
    E: From<git::GitError> + From<lock::LockError>,
{
    // 耗时起点在取锁之前：锁上等待也算这次命令的耗时
    let started = Instant::now();
    let paths = HubPaths::from_root(hub_root);
    let token = lock::write_lock(&paths)?;
    let mut ctx = Ctx::new(&token, &paths, OnFailure::Resume);
    // 仓库已在就先封存手改现场；首建的仓库还没有，由落盘段建出后交回
    if paths.root().join(".git").exists() {
        let repo = git::Repository::open(paths.root()).map_err(git::GitError::from)?;
        git::sync_manual(&repo);
        ctx.attach_repo(repo);
    }
    let plan = plan(&paths)?;
    land(ctx, plan, apply, milestone, started)
}

/// 落盘段：按计划落盘 → 机器提交 → 里程碑日志。开仓与封存由两种姿势各自做完，
/// 这里是两种姿势共用的唯一落盘与提交实现。提交清单先于落盘从计划读出：
/// 落盘会改动计划里的执行数据（落点档位、逐目标结果），提交步不随之改变
fn land<Plan, Outcome, Milestone, E>(
    mut ctx: Ctx<'_>,
    plan: Plan,
    apply: impl FnOnce(&mut Ctx<'_>, Plan) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
    started: Instant,
) -> Result<Outcome, E>
where
    Plan: Steps,
{
    // 计划里的提交步：一条消息一个提交，顺序即计划顺序。没有提交步表示本次
    // 没有可提交的变更，`auto_commit` 不必惊动。提交步缺消息是计划构造的缺陷，
    // 不能静默少提交一次，记 ERROR 留痕
    let mut commits: Vec<String> = Vec::new();
    for step in plan.steps() {
        if step.action != crate::plan::ActionKind::Commit {
            continue;
        }
        match step.target {
            Some(message) => commits.push(message),
            None => tracing::error!(action = ?step.action, "提交步缺消息"),
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::*;
    use crate::commit::{Face, Verb, message};
    use crate::plan::PlanStep;

    /// 一次性 hub（初始化完成）与它的根路径
    fn setup() -> (tempfile::TempDir, PathBuf) {
        let install = tempfile::tempdir().unwrap();
        crate::hub::init(install.path(), false).unwrap();
        let root = install.path().to_path_buf();
        (install, root)
    }

    /// 回滚策略：落盘失败时逆序撤销已登记的盘上动作，错误原样上抛
    #[test]
    fn rollback_policy_undoes_registered_actions_in_reverse() {
        let (_install, root) = setup();
        let seen: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
        let result: Result<(), CommonError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |_paths| Ok(Vec::new()),
            |_plan| (),
            |ctx, _plan: Vec<PlanStep>| {
                for step in [1, 2] {
                    let seen = Rc::clone(&seen);
                    ctx.on_rollback(move || seen.borrow_mut().push(step));
                }
                Err(CommonError::from(std::io::Error::other("落盘失败")))
            },
            |(), _elapsed| panic!("落盘失败不得记里程碑"),
        );
        assert!(result.is_err(), "落盘失败要原样上抛");
        assert_eq!(*seen.borrow(), vec![2, 1], "撤销按登记的逆序执行");
    }

    /// 续跑策略：落盘失败时不碰盘面，已做的部分留给重跑续完
    #[test]
    fn resume_policy_keeps_partial_disk_state() {
        let (_install, root) = setup();
        let seen: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
        let result: Result<(), CommonError> = run(
            &root,
            false,
            OnFailure::Resume,
            |_paths| Ok(Vec::new()),
            |_plan| (),
            |ctx, _plan: Vec<PlanStep>| {
                let seen = Rc::clone(&seen);
                ctx.on_rollback(move || seen.borrow_mut().push(1));
                Err(CommonError::from(std::io::Error::other("落盘失败")))
            },
            |(), _elapsed| panic!("落盘失败不得记里程碑"),
        );
        assert!(result.is_err());
        assert!(seen.borrow().is_empty(), "续跑策略不得撤销已做的动作");
    }

    /// 预演分支：只读开仓、只算计划，不落盘不取锁——别人持锁时照样出计划
    #[test]
    fn dry_run_plans_without_lock_or_disk() {
        let (_install, root) = setup();
        let paths = HubPaths::from_root(&root);
        // 模拟并发写：先取全局写锁且不放手
        let _held = crate::lock::write_lock(&paths).unwrap();

        let plan = vec![PlanStep::commit("add(store): demo")];
        let outcome: Result<Vec<PlanStep>, CommonError> = run(
            &root,
            true,
            OnFailure::Rollback,
            |_paths| Ok(plan.clone()),
            |dry| dry.all_steps(),
            |_ctx, _plan| panic!("预演不得落盘"),
            |_milestone: (), _elapsed| panic!("预演不得记里程碑"),
        );
        assert_eq!(outcome.unwrap(), plan, "预演交回的清单就是计划本身");
    }

    /// 实跑分支：落盘成功后按计划里的提交步逐条提交，里程碑拿到骨架计的耗时
    #[test]
    fn write_run_commits_each_message_and_logs_elapsed() {
        let (_install, root) = setup();
        let logged: RefCell<Vec<u64>> = RefCell::new(Vec::new());
        let head_before = git::Repository::open(&root)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap();

        let outcome: Result<PathBuf, CommonError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |_paths| {
                Ok(vec![PlanStep::commit(message(
                    Verb::Add,
                    Face::Store,
                    "demo",
                ))])
            },
            |_plan| panic!("实跑不得走预演出口"),
            |ctx, _plan: Vec<PlanStep>| {
                // 落盘：写一个文件，随后由骨架按计划里的提交步统一提交
                let dir = ctx.paths.skill_workdir("demo");
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join("SKILL.md"), "正文")?;
                Ok(Applied {
                    outcome: dir,
                    milestones: (),
                })
            },
            |(), elapsed| {
                logged.borrow_mut().push(elapsed);
            },
        );
        let dir = outcome.unwrap();
        assert_eq!(logged.borrow().len(), 1, "里程碑只记一次");
        assert_eq!(
            std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
            "正文",
            "落盘结果留在盘上"
        );
        let repo = git::Repository::open(&root).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_ne!(head.id(), head_before, "落盘成功后要有机器提交");
        assert_eq!(
            head.message().unwrap(),
            "add(store): demo",
            "提交消息取自计划的提交步"
        );
        assert!(!git::is_dirty(&repo).unwrap(), "提交后工作区应干净");
    }

    /// 空计划：没有提交步就不动 hub 仓库，不留空提交
    #[test]
    fn write_run_without_commits_leaves_head_alone() {
        let (_install, root) = setup();
        let head_before = git::Repository::open(&root)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap();
        let outcome: Result<(), CommonError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |_paths| Ok(Vec::new()),
            |_plan| (),
            |_ctx, _plan: Vec<PlanStep>| {
                Ok(Applied {
                    outcome: (),
                    milestones: (),
                })
            },
            |(), _elapsed| (),
        );
        outcome.unwrap();
        assert_eq!(
            git::Repository::open(&root)
                .unwrap()
                .head()
                .unwrap()
                .target()
                .unwrap(),
            head_before,
            "无提交步时不得产生提交"
        );
    }
}
