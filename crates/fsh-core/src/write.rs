//! 写入口共用骨架：hub 写命令的同一套前导与尾声。
//! 入口先做不依赖 hub 的入参判定，随后骨架接手：装配判定 → 开仓（预演只读、
//! 实跑持写锁并封存手改现场）→ 算计划 → 落盘 → 机器提交 → 耗时与里程碑日志。
//! 入库、归档、移除、恢复、使用、撤回六个写入口因此只剩「计划」与「落盘」，
//! 锁姿势、预演分叉与日志形状都只写一遍。
//!
//! 落盘失败时盘面怎么处置由 [`OnFailure`] 显式点名，不藏在各入口的分支里：
//! 回滚（撤销本次已登记的盘上动作）或续跑（保留中途盘面，重跑续完）。
//! 锁内顺序不变量（先取锁 → 再读登记簿 → 最后写盘）由 [`Ctx`] 的字段保证：
//! 登记簿写回只能拿到 `token`，机器提交只经骨架，入口碰不到仓库句柄。

use std::path::Path;
use std::time::Instant;

use crate::error::CommonError;
use crate::git;
use crate::lock::WriteToken;
use crate::paths::HubPaths;

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
}

impl Ctx<'_> {
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

/// 一次落盘交回骨架的三样东西：命令报告本体、要机器提交的消息、里程碑数据。
/// 提交消息逐条对应一次提交，空清单表示本次在 hub 仓库上无可提交的变更
pub(crate) struct Applied<Outcome, Milestone> {
    /// 命令的结果载荷
    pub(crate) outcome: Outcome,
    /// 机器提交消息，一条一次提交
    pub(crate) commits: Vec<String>,
    /// 记里程碑日志要用的数据，由入口自己定义形状
    pub(crate) milestones: Milestone,
}

/// 一次写命令的执行：装配判定 → 开仓 → 计划 → 落盘 → 提交与里程碑。
/// 入参判定（键形状、目标集合非空）由入口先做：它们不依赖 hub，
/// 先报出来才不会落到「hub 未初始化」。预演只读开仓、只算计划；实跑持锁落盘。
/// 落盘成功后按 `apply` 给出的消息逐条机器提交，再拿 `apply` 交回的里程碑数据
/// 记日志；落盘失败按 `failure` 处置盘面后原样上抛，错误只在入口边界记一次
pub(crate) fn run<Plan, Outcome, Milestone, E>(
    hub_root: &Path,
    dry_run: bool,
    failure: OnFailure,
    plan: impl FnOnce(&HubPaths) -> Result<Plan, E>,
    dry_outcome: impl FnOnce(Plan) -> Outcome,
    apply: impl FnOnce(&mut Ctx<'_>, Plan) -> Result<Applied<Outcome, Milestone>, E>,
    milestone: impl FnOnce(Milestone, u64),
) -> Result<Outcome, E>
where
    E: From<CommonError>,
{
    if dry_run {
        // 预演：装配判定 + 只读开仓，不取写锁——别人正在写也能拿到动作清单
        git::open_hub(hub_root).map_err(CommonError::from)?;
        let paths = HubPaths::from_root(hub_root);
        let plan = plan(&paths)?;
        return Ok(dry_outcome(plan));
    }
    // 实跑：装配判定、取全局写锁、开仓、封存手改现场由这一处一次做完。
    // 耗时起点在取锁之前：锁上等待也算这次命令的耗时
    let started = Instant::now();
    let (token, repo, paths) = git::open_hub_for_write(hub_root)?;
    let plan = plan(&paths)?;
    let mut ctx = Ctx {
        token: &token,
        paths: &paths,
        failure,
        undo: Vec::new(),
    };
    let Applied {
        outcome,
        commits,
        milestones,
    } = match apply(&mut ctx, plan) {
        Ok(done) => done,
        Err(e) => {
            // 落盘失败：按策略处置盘面，再把原错误交给入口边界
            ctx.settle();
            return Err(e);
        }
    };
    // 落盘成功后的机器提交：一条消息一次提交，空清单表示本次无可提交的变更
    for message in &commits {
        git::auto_commit(&repo, message);
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
            |_paths| Ok(()),
            |()| (),
            |ctx, (): ()| {
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
            |_paths| Ok(()),
            |()| (),
            |ctx, (): ()| {
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

        let outcome: Result<String, CommonError> = run(
            &root,
            true,
            OnFailure::Rollback,
            |_paths| Ok("计划".to_string()),
            |plan| plan,
            |_ctx, _plan| panic!("预演不得落盘"),
            |_milestone: (), _elapsed| panic!("预演不得记里程碑"),
        );
        assert_eq!(outcome.unwrap(), "计划");
    }

    /// 实跑分支：落盘成功后逐条消息提交，里程碑拿到骨架计的耗时
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
            |_paths| Ok("demo".to_string()),
            |_plan| panic!("实跑不得走预演出口"),
            |ctx, plan: String| {
                // 落盘：写一个文件，随后由骨架统一提交
                let dir = ctx.paths.skill_workdir(&plan);
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join("SKILL.md"), "正文")?;
                Ok(Applied {
                    outcome: dir,
                    commits: vec![message(Verb::Add, Face::Store, &plan)],
                    milestones: plan,
                })
            },
            |plan, elapsed| {
                assert_eq!(plan, "demo", "里程碑拿到本次计划的对象");
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
        assert_ne!(
            repo.head().unwrap().target().unwrap(),
            head_before,
            "落盘成功后要有机器提交"
        );
        assert!(!git::is_dirty(&repo).unwrap(), "提交后工作区应干净");
    }

    /// 空提交清单：落盘不动 hub 仓库时不留空提交
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
            |_paths| Ok(()),
            |()| (),
            |_ctx, (): ()| {
                Ok(Applied {
                    outcome: (),
                    commits: Vec::new(),
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
            "无提交消息时不得产生提交"
        );
    }
}
