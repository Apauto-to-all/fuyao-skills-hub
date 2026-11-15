//! 写入口共用骨架：hub 写命令的同一套前导与尾声。
//! 入口先做不依赖 hub 的入参判定，随后骨架接手：装配判定 → 开仓（预演只读、
//! 实跑持写锁并封存手改现场）→ 算计划 → 落盘 → 机器提交 → 耗时与里程碑日志。
//! 入库、归档、移除、恢复、使用、撤回六个写入口因此只剩「一份计划」与
//! 「一个落盘函数」，锁姿势、预演分叉与日志形状都只写一遍。
//!
//! 开仓有两种姿势，落盘段只有一份：写入口要求装配完整（[`run`]），
//! 装配入口在仓库尚未建出时也要能开工（[`run_assembly`]）。两种姿势都先取
//! 全局写锁、再封存手改现场，随后交给同一段落盘：按计划落盘 → 机器提交
//! → 里程碑日志。首建的仓库由落盘段建出后经 [`Ctx::attach_repo`] 交回骨架。
//!
//! 盘面动作一律经 [`Ctx`] 的原语做（移动 / 复制 / 删除 / 建目录 / 写文件 /
//! 建落点 / 撤落点 / 重链）：原语在动手那一刻自己登记撤销，入口没有别的地方
//! 可登记，也没有别的地方会漏登记。撤销清单因而只反映真做过的动作，
//! 落盘失败时按登记的逆序一次撤干净。
//!
//! 机器提交按计划的提交步落：提交清单在落盘之前读出，落盘段只负责执行它。
//! 预演清单与实跑提交因此同出一份计划，命令只说一次自己要提交什么。
//!
//! 落盘失败时盘面怎么处置由 [`OnFailure`] 显式点名，不藏在各入口的分支里：
//! 回滚（撤销本次已登记的盘上动作）或续跑（保留中途盘面，重跑续完）。
//! 锁内顺序不变量（先取锁 → 再读登记簿 → 最后写盘）由 [`Ctx`] 的字段保证：
//! 登记簿写回只能拿到 `token`，机器提交只经骨架，入口碰不到仓库句柄。

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::error::CommonError;
use crate::git;
use crate::hub;
use crate::link::{self, LinkError, LinkKind};
use crate::lock::{self, WriteToken};
use crate::paths::HubPaths;
use crate::plan::{Act, Plan, PlanStep, Step};

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
    token: &'a WriteToken,
    /// hub 路径族
    paths: &'a HubPaths,
    /// 失败时的盘面处置
    failure: OnFailure,
    /// 本次已做盘上动作的撤销清单，按登记的逆序执行
    undo: Vec<Box<dyn FnOnce()>>,
    /// 本入口的仓库句柄：装配入口在建出仓库之前就开工，故由落盘段交回
    repo: Option<git::Repository>,
    /// 登记簿缓存：一次落盘只读一次，写回成功后随之更新
    registry: Option<hub::ProjectRegistry>,
    /// 本次落盘建出的落点与实际档位：逐目标收口链接闸从这里取
    landed_links: Vec<(PathBuf, LinkKind)>,
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
            registry: None,
            landed_links: Vec::new(),
        }
    }

    /// hub 路径族：只读方取用；动盘一律走本现场的动作
    pub(crate) fn paths(&self) -> &HubPaths {
        self.paths
    }

    /// 交回仓库句柄：首建的装配入口在落盘段里才把仓库建出来
    pub(crate) fn attach_repo(&mut self, repo: git::Repository) {
        self.repo = Some(repo);
    }

    /// 按计划顺序执行盘面动作：一步一条，撤销由各原语就地登记
    pub(crate) fn land<E>(&mut self, steps: &[Step]) -> Result<(), E>
    where
        E: From<std::io::Error> + From<LinkError>,
    {
        for step in steps {
            match &step.act {
                Act::None => {}
                Act::Move { from, to } => self.move_dir(from, to)?,
                Act::Copy { from, to } => self.copy_dir(from, to)?,
                Act::Remove(dir) => self.remove_dir(dir)?,
                Act::Link { from, to } => {
                    self.link_dir(from, to)?;
                }
                Act::Unlink { path, kind } => self.unlink(path, *kind)?,
                Act::Relink {
                    path,
                    from,
                    to,
                    kind,
                } => self.relink(path, *kind, from, to)?,
            }
        }
        Ok(())
    }

    /// 本次落盘建在这个落点上的档位：没建过就是没有
    pub(crate) fn link_kind(&self, dst: &Path) -> Option<LinkKind> {
        self.landed_links
            .iter()
            .find(|(path, _)| path == dst)
            .map(|(_, kind)| *kind)
    }

    /// 登记一条撤销动作：落盘失败且策略为回滚时由骨架逆序执行。
    /// 撤销动作自己负责在删不掉时留痕，骨架只管按序调用。
    /// 只给本现场的原语用——盘面动作一律经原语做，登记点因此不会漏
    fn on_rollback(&mut self, undo: impl FnOnce() + 'static) {
        self.undo.push(Box::new(undo));
    }

    /// 挪目录：父层目录建出 → 改名 → 就地登记「改名回来」。
    /// 改名没成就没有可撤的动作，故只在改名成功后登记
    pub(crate) fn move_dir(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(from, to)?;
        let (back_to, moved_from) = (from.to_path_buf(), to.to_path_buf());
        self.on_rollback(move || {
            if let Err(e) = std::fs::rename(&moved_from, &back_to) {
                tracing::warn!(path = %moved_from.display(), cause = %e, "回滚目录挪动失败");
            }
        });
        Ok(())
    }

    /// 复制目录树（任何深度的 `.git` 都不带入）：先建立点并登记撤销，
    /// 再逐项拷贝——拷到一半失败时，已写下的半棵树同样要删净
    pub(crate) fn copy_dir(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        let undo = to.to_path_buf();
        self.on_rollback(move || {
            if undo.exists()
                && let Err(e) = std::fs::remove_dir_all(&undo)
            {
                tracing::warn!(path = %undo.display(), cause = %e, "回滚复制的目录删除失败");
            }
        });
        copy_tree(from, to)?;
        Ok(())
    }

    /// 删目录：删除不可逆，已删的部分保持删掉，配 [`OnFailure::Resume`] 重跑续完
    pub(crate) fn remove_dir(&mut self, dir: &Path) -> std::io::Result<()> {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// 建目录：空目录不进提交，重跑无副作用，故不登记撤销
    pub(crate) fn create_dir(&mut self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        Ok(())
    }

    /// 写文件：父层目录建出 → 登记「写回原内容 / 删掉新建的」→ 写。
    /// 先登记再写，写到一半失败也能把原内容摆回去
    pub(crate) fn write_file(&mut self, path: &Path, body: &[u8]) -> std::io::Result<()> {
        let previous = std::fs::read(path).ok();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let undo_path = path.to_path_buf();
        self.on_rollback(move || match previous {
            Some(bytes) => {
                if let Err(e) = std::fs::write(&undo_path, bytes) {
                    tracing::warn!(path = %undo_path.display(), cause = %e, "回滚文件写回失败");
                }
            }
            None => {
                if let Err(e) = std::fs::remove_file(&undo_path)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(path = %undo_path.display(), cause = %e, "回滚新建文件删除失败");
                }
            }
        });
        std::fs::write(path, body)?;
        Ok(())
    }

    /// 建落点：建出指向 `from` 的入口并就地登记「删掉它」，档位当场记下
    pub(crate) fn link_dir(&mut self, from: &Path, to: &Path) -> Result<LinkKind, LinkError> {
        let kind = match link::create_dir_link(from, to) {
            Ok(kind) => kind,
            Err(e) => {
                // 建链接失败是本次使用的失败边界：记下哪个落点失败与原因，随后整批回滚
                tracing::warn!(path = %to.display(), cause = %e, "落点建链接失败");
                return Err(e);
            }
        };
        let undo = to.to_path_buf();
        self.on_rollback(move || {
            // 回滚删不掉会留下半套入口，是唯一能发现残留的地方，必须留痕
            if let Err(e) = link::remove_link(&undo, kind) {
                tracing::warn!(path = %undo.display(), cause = %e, "回滚落点删除失败");
            }
        });
        self.landed_links.push((to.to_path_buf(), kind));
        Ok(kind)
    }

    /// 撤落点：按登记的档位删，目标不在场视为已撤销（幂等）。
    /// 删除不可逆，配 [`OnFailure::Resume`] 重跑续完
    pub(crate) fn unlink(&mut self, path: &Path, kind: LinkKind) -> Result<(), LinkError> {
        if let Err(e) = link::remove_link(path, kind) {
            tracing::warn!(path = %path.display(), cause = %e, "落点删除失败");
            return Err(e);
        }
        Ok(())
    }

    /// 换源重链：删掉 `path` 上的旧入口，按同一档位重建指向 `to` 的入口。
    /// 撤销先登记再建：删新链、按 `from` 重建旧链，建到一半失败也能换回去
    pub(crate) fn relink(
        &mut self,
        path: &Path,
        kind: LinkKind,
        from: &Path,
        to: &Path,
    ) -> Result<(), LinkError> {
        link::remove_link(path, kind)?;
        let (undo_path, old_source) = (path.to_path_buf(), from.to_path_buf());
        self.on_rollback(move || {
            if let Err(e) = link::remove_link(&undo_path, kind) {
                tracing::warn!(path = %undo_path.display(), cause = %e, "回滚重链落点删除失败");
                return;
            }
            if let Err(e) = link::create_dir_link(&old_source, &undo_path) {
                tracing::warn!(path = %undo_path.display(), cause = %e, "回滚重链落点重建失败");
            }
        });
        link::create_dir_link(to, path)?;
        self.landed_links.push((path.to_path_buf(), kind));
        Ok(())
    }

    /// 改登记簿：读一次（缓存）→ 改 → 原子写回。登记簿只有这一个写入口，
    /// 写回失败时缓存不更新，盘上与缓存都不留半套改动
    pub(crate) fn edit_registry<E>(
        &mut self,
        mutate: impl FnOnce(&mut hub::ProjectRegistry) -> Result<(), E>,
    ) -> Result<(), E>
    where
        E: From<CommonError>,
    {
        let mut registry = match self.registry.take() {
            Some(cached) => cached,
            None => hub::load_registry(self.paths).map_err(CommonError::from)?,
        };
        mutate(&mut registry)?;
        hub::save_registry(self.paths, &registry, self.token).map_err(CommonError::from)?;
        self.registry = Some(registry);
        Ok(())
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

/// 递归复制目录树，跳过任何深度的 .git 目录（源自身是仓库时不带入嵌套仓库）
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
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
/// 故不走写入口的装配判定。取锁 → 仓库已在就开出来并封存手改 → 算计划 →
/// 落盘 → 机器提交 → 里程碑；首建的仓库由落盘段经 [`Ctx::attach_repo`] 交回。
/// 盘面一律留在中途等重跑续完：装配的每一步都幂等，重跑即续完
pub(crate) fn run_assembly<T, Outcome, Milestone, E>(
    hub_root: &Path,
    plan: impl FnOnce(&HubPaths) -> Result<Plan<T>, E>,
    apply: impl FnOnce(&mut Ctx<'_>, Plan<T>) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
) -> Result<Outcome, E>
where
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use super::*;
    use crate::commit::{Face, Verb, message};
    use crate::plan::ActionKind;
    use crate::project::ProjectError;

    /// 一次性 hub（初始化完成）与它的根路径
    fn setup() -> (tempfile::TempDir, PathBuf) {
        let install = tempfile::tempdir().unwrap();
        crate::hub::init(install.path(), false).unwrap();
        let root = install.path().to_path_buf();
        (install, root)
    }

    /// 一次空计划：预演出口直出描述，落盘函数与里程碑都不许被调用
    fn dry_run_plan(root: &Path) -> Result<Vec<PlanStep>, ProjectError> {
        run(
            root,
            true,
            OnFailure::Rollback,
            |_paths| {
                let mut plan = Plan::new(());
                plan.push(PlanStep::commit("add(store): demo"), Act::None);
                Ok(plan)
            },
            |plan, command| {
                let mut all = command;
                all.extend(plan.describe());
                all
            },
            |_ctx, _plan| panic!("预演不得落盘"),
            |_milestone: (), _elapsed| panic!("预演不得记里程碑"),
        )
    }

    /// 计划里的盘面动作由骨架按序执行：落盘函数只管业务部分。
    /// 复制不带入任何深度的 .git
    #[test]
    fn plan_acts_are_landed_by_the_skeleton() {
        let (_install, root) = setup();
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("SKILL.md"), "正文").unwrap();
        std::fs::create_dir_all(src.path().join("refs")).unwrap();
        std::fs::write(src.path().join("refs/a.txt"), "引用").unwrap();
        std::fs::create_dir_all(src.path().join(".git")).unwrap();
        std::fs::write(src.path().join(".git/config"), "不应带入").unwrap();

        let outcome: Result<PathBuf, ProjectError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |paths| {
                let dst = paths.skill_workdir("demo");
                let mut plan = Plan::new(dst.clone());
                plan.push(
                    PlanStep::between(ActionKind::Copy, "src", "store/demo").targeting("demo"),
                    Act::Copy {
                        from: src.path().to_path_buf(),
                        to: dst,
                    },
                );
                Ok(plan)
            },
            |_plan, _command| panic!("实跑不得走预演出口"),
            |_ctx, plan| {
                Ok(Applied {
                    outcome: plan.data,
                    milestones: (),
                })
            },
            |(), _elapsed| (),
        );
        let dst = outcome.unwrap();
        assert_eq!(
            std::fs::read_to_string(dst.join("SKILL.md")).unwrap(),
            "正文",
            "计划的复制动作已由骨架落盘"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("refs/a.txt")).unwrap(),
            "引用",
            "子目录一并复制"
        );
        assert!(!dst.join(".git").exists(), "任何深度的 .git 都不带入");
    }

    /// 回滚策略：按登记的逆序撤销已做动作，错误原样上抛。
    /// 两段挪动套着做，逆序撤销才回得到原处
    #[test]
    fn rollback_policy_undoes_landed_acts_in_reverse() {
        let (_install, root) = setup();
        // 开工前的盘面：工作副本在场，挪动从这里出发
        let demo = HubPaths::from_root(&root).skill_workdir("demo");
        std::fs::create_dir_all(&demo).unwrap();
        std::fs::write(demo.join("SKILL.md"), "正文").unwrap();
        let result: Result<(), ProjectError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |paths| {
                let demo = paths.skill_workdir("demo");
                let first = paths.skill_workdir("x");
                let second = paths.skill_workdir("y");
                let mut plan = Plan::new(());
                plan.push(
                    PlanStep::between(ActionKind::Move, "store/demo", "store/x"),
                    Act::Move {
                        from: demo.clone(),
                        to: first.clone(),
                    },
                );
                plan.push(
                    PlanStep::between(ActionKind::Move, "store/x", "store/y"),
                    Act::Move {
                        from: first,
                        to: second,
                    },
                );
                // 第三步的源不存在：落盘在这里失败，前两步要走撤销
                plan.push(
                    PlanStep::between(ActionKind::Move, "store/absent", "store/z"),
                    Act::Move {
                        from: paths.skill_workdir("absent"),
                        to: paths.skill_workdir("z"),
                    },
                );
                Ok(plan)
            },
            |_plan, _command| panic!("实跑不得走预演出口"),
            |_ctx, _plan| panic!("落盘动作失败不进落盘函数"),
            |(), _elapsed| panic!("落盘失败不得记里程碑"),
        );
        assert!(result.is_err(), "落盘失败要原样上抛");
        assert_eq!(
            std::fs::read_to_string(
                HubPaths::from_root(&root)
                    .skill_workdir("demo")
                    .join("SKILL.md")
            )
            .unwrap(),
            "正文",
            "挪动按逆序撤销，内容回得到原处"
        );
        assert!(
            !HubPaths::from_root(&root).skill_workdir("x").exists(),
            "中途目录不留残影"
        );
        assert!(
            !HubPaths::from_root(&root).skill_workdir("y").exists(),
            "中途目录不留残影"
        );
    }

    /// 续跑策略：落盘失败时不碰盘面，已做的部分留给重跑续完
    #[test]
    fn resume_policy_keeps_landed_acts() {
        let (_install, root) = setup();
        let paths = HubPaths::from_root(&root);
        std::fs::create_dir_all(paths.skill_workdir("demo")).unwrap();
        let result: Result<(), ProjectError> = run(
            &root,
            false,
            OnFailure::Resume,
            |paths| {
                let demo = paths.skill_workdir("demo");
                let mut plan = Plan::new(());
                plan.push(
                    PlanStep::between(ActionKind::Move, "store/demo", "store/x"),
                    Act::Move {
                        from: demo,
                        to: paths.skill_workdir("x"),
                    },
                );
                plan.push(
                    PlanStep::between(ActionKind::Move, "store/absent", "store/y"),
                    Act::Move {
                        from: paths.skill_workdir("absent"),
                        to: paths.skill_workdir("y"),
                    },
                );
                Ok(plan)
            },
            |_plan, _command| panic!("实跑不得走预演出口"),
            |_ctx, _plan| panic!("落盘动作失败不进落盘函数"),
            |(), _elapsed| panic!("落盘失败不得记里程碑"),
        );
        assert!(result.is_err());
        assert!(paths.skill_workdir("x").exists(), "续跑策略保留已做的挪动");
        assert!(!paths.skill_workdir("demo").exists(), "已做的挪动不回退");
    }

    /// 写文件原语：回滚时把原内容写回去，新建的文件删掉
    #[test]
    fn write_file_rollback_restores_previous_bytes() {
        let (_install, root) = setup();
        let existing = root.join("existing.txt");
        std::fs::write(&existing, "旧").unwrap();
        let created = root.join("created.txt");
        let result: Result<(), ProjectError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |_paths| Ok(Plan::new(())),
            |_plan, _command| panic!("实跑不得走预演出口"),
            |ctx, _plan| {
                ctx.write_file(&existing, "新".as_bytes())?;
                ctx.write_file(&created, "新".as_bytes())?;
                Err(ProjectError::from(std::io::Error::other("落盘失败")))
            },
            |(), _elapsed| panic!("落盘失败不得记里程碑"),
        );
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&existing).unwrap(),
            "旧",
            "原内容写回"
        );
        assert!(!created.exists(), "新建的文件删掉");
    }

    /// 建落点原语：实际采用的档位当场记下，逐目标收口链接闸从这里取
    #[test]
    fn link_kind_is_recorded_for_landed_links() {
        let (_install, root) = setup();
        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("SKILL.md"), "正文").unwrap();
        let dest = root.join("dest/demo");
        let outcome: Result<Option<LinkKind>, ProjectError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |_paths| {
                let mut plan = Plan::new(dest.clone());
                plan.push(
                    PlanStep::between(ActionKind::Link, "store/demo", "dest/demo"),
                    Act::Link {
                        from: source.path().to_path_buf(),
                        to: dest.clone(),
                    },
                );
                Ok(plan)
            },
            |_plan, _command| panic!("实跑不得走预演出口"),
            |ctx, plan| {
                Ok(Applied {
                    outcome: ctx.link_kind(&plan.data),
                    milestones: (),
                })
            },
            |(), _elapsed| (),
        );
        assert!(outcome.unwrap().is_some(), "建出的落点要记下实际档位");
        assert!(dest.exists(), "落点已建出");
    }

    /// 预演分支：只读开仓、只算计划，不落盘不取锁——别人持锁时照样出计划
    #[test]
    fn dry_run_plans_without_lock_or_disk() {
        let (_install, root) = setup();
        // 模拟并发写：先取全局写锁且不放手
        let _held = crate::lock::write_lock(&HubPaths::from_root(&root)).unwrap();
        assert_eq!(
            dry_run_plan(&root).unwrap(),
            vec![PlanStep::commit("add(store): demo")],
            "预演交回的清单就是计划本身"
        );
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

        let outcome: Result<PathBuf, ProjectError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |paths| {
                let dir = paths.skill_workdir("demo");
                let mut plan = Plan::new(dir.clone());
                plan.push(
                    PlanStep::commit(message(Verb::Add, Face::Store, "demo")),
                    Act::None,
                );
                Ok(plan)
            },
            |_plan, _command| panic!("实跑不得走预演出口"),
            |ctx, plan| {
                // 落盘：写一个文件，随后由骨架按计划里的提交步统一提交
                ctx.write_file(&plan.data.join("SKILL.md"), "正文".as_bytes())?;
                Ok(Applied {
                    outcome: plan.data,
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
        let outcome: Result<(), ProjectError> = run(
            &root,
            false,
            OnFailure::Rollback,
            |_paths| Ok(Plan::new(())),
            |_plan, _command| panic!("实跑不得走预演出口"),
            |_ctx, _plan| {
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
