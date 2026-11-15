use std::cell::RefCell;
use std::path::PathBuf;

use super::*;
use crate::commit::{Face, Verb, message};
use crate::link::LinkKind;
use crate::plan::{Act, ActionKind};
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

/// 建落点原语：组里第一个落点定档并记下，登记记录从这里取
#[test]
fn group_kind_is_recorded_for_landed_links() {
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
                    group: 0,
                },
            );
            Ok(plan)
        },
        |_plan, _command| panic!("实跑不得走预演出口"),
        |ctx, _plan| {
            Ok(Applied {
                outcome: ctx.group_kind(0),
                milestones: (),
            })
        },
        |(), _elapsed| (),
    );
    assert!(outcome.unwrap().is_some(), "建出的落点要记下实际档位");
    assert!(dest.exists(), "落点已建出");
}

/// 重链按钉住的档位重建：登记是 copy 就还是真目录，不升成链接
#[test]
fn relink_rebuilds_the_pinned_kind() {
    let (_install, root) = setup();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("SKILL.md"), "正文").unwrap();
    let old_source = tempfile::tempdir().unwrap();
    std::fs::write(old_source.path().join("SKILL.md"), "旧正文").unwrap();
    let dest = root.join("dest/demo");
    let outcome: Result<(), ProjectError> = run(
        &root,
        false,
        OnFailure::Rollback,
        |_paths| {
            let mut plan = Plan::new(dest.clone());
            plan.push(
                PlanStep::between(ActionKind::Link, "store/demo", "dest/demo"),
                Act::Link {
                    from: old_source.path().to_path_buf(),
                    to: dest.clone(),
                    group: 0,
                },
            );
            Ok(plan)
        },
        |_plan, _command| panic!("实跑不得走预演出口"),
        |ctx, plan| {
            // 登记写的是 copy：重链后落点必须还是真目录，不能挑降级链上更省事的一级
            ctx.relink(&plan.data, LinkKind::Copy, old_source.path(), source.path())?;
            Ok(Applied {
                outcome: (),
                milestones: (),
            })
        },
        |(), _elapsed| (),
    );
    outcome.unwrap();
    let meta = std::fs::symlink_metadata(&dest).unwrap();
    assert!(meta.is_dir(), "钉住的 copy 档要落成真目录");
    assert!(!meta.file_type().is_symlink(), "不得悄悄升成链接");
    assert_eq!(
        std::fs::read_to_string(dest.join("SKILL.md")).unwrap(),
        "正文",
        "内容取自新源"
    );
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
