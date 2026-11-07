//! 多目标撤回测试：目标集合的合法性、全局与项目同一次调用、任一目标不合格时整次不动盘。

use super::*;
use crate::hub;

#[test]
fn unuse_without_any_target_is_rejected() {
    let (_install, paths) = setup_with_skill(None);
    let err = unuse(paths.root(), "demo", Targets::default(), None, false, false).unwrap_err();
    assert_eq!(err.error_code(), "no_target");
}

#[test]
fn unuse_rejects_a_project_that_does_not_exist() {
    let (_install, paths) = setup_with_skill(None);
    let missing = tempfile::tempdir().unwrap().path().join("gone");
    let err = unuse(
        paths.root(),
        "demo",
        Targets::project(&missing),
        None,
        false,
        false,
    )
    .unwrap_err();
    // 目录不存在是目标本身不合格，不该伪装成「没有登记记录」
    assert_eq!(err.error_code(), "invalid_project");
}

#[test]
fn unuse_global_removes_global_destinations_and_record() {
    let (_install, paths) = setup_with_skill(None);
    let global_dir = tempfile::tempdir().unwrap();
    let agent = global_agent(&paths, global_dir.path());
    let used = use_skill(
        paths.root(),
        "demo",
        None,
        std::slice::from_ref(&agent),
        Targets::global(),
        false,
    )
    .unwrap();
    assert!(global_dir.path().join("demo").exists());

    let r = unuse(paths.root(), "demo", Targets::global(), None, false, false).unwrap();
    assert_eq!(r.targets[0].scope, crate::hub::Scope::Global);
    assert!(r.targets[0].project.is_none());
    assert_eq!(r.targets[0].removed.len(), 1);
    assert_eq!(r.targets[0].agents, vec![agent.clone()]);
    // 撤回报告带出登记时的链接方式，与使用报告同源
    assert_eq!(r.targets[0].link, used.targets[0].link);
    assert!(!global_dir.path().join("demo").exists());
    let reg = hub::load_registry(&paths).unwrap();
    assert!(reg.global().skills.is_empty(), "全局记录应删除");
}

#[test]
fn unuse_serves_global_and_projects_in_one_call_without_touching_others() {
    let (_install, paths) = setup_with_skill(None);
    let global_dir = tempfile::tempdir().unwrap();
    let global_only = global_agent(&paths, global_dir.path());
    let first = temp_project();
    let second = temp_project();
    let project_agent = vec!["universal".to_string()];
    // 全局与两个项目各用一次 use：全局用只有全局位的 agent，项目用有项目位的 agent
    use_skill(
        paths.root(),
        "demo",
        None,
        &[global_only],
        Targets::global(),
        false,
    )
    .unwrap();
    use_skill(
        paths.root(),
        "demo",
        None,
        &project_agent,
        Targets::new(false, vec![first.clone(), second.clone()]),
        false,
    )
    .unwrap();

    // 只撤第二个项目：全局与第一个项目一字不动
    unuse(
        paths.root(),
        "demo",
        Targets::project(second.as_path()),
        None,
        false,
        false,
    )
    .unwrap();
    assert!(global_dir.path().join("demo").exists(), "全局落点不得被撤");
    assert!(
        first.join(".agents/skills/demo").exists(),
        "第一个项目不得被撤"
    );
    assert!(record(&paths, first.as_path(), "demo").is_some());
    assert!(record(&paths, second.as_path(), "demo").is_none());
    assert_eq!(hub::load_registry(&paths).unwrap().global().skills.len(), 1);

    // 全局与第一个项目一起撤：一次调用撤净两处
    let r = unuse(
        paths.root(),
        "demo",
        Targets::new(true, vec![first.clone()]),
        None,
        false,
        false,
    )
    .unwrap();
    assert_eq!(r.targets.len(), 2);
    assert_eq!(r.targets[0].scope, crate::hub::Scope::Global);
    assert_eq!(r.targets[1].scope, crate::hub::Scope::Project);
    assert!(!global_dir.path().join("demo").exists());
    assert!(!first.join(".agents/skills/demo").exists());
    let reg = hub::load_registry(&paths).unwrap();
    assert!(reg.global().skills.is_empty());
    assert!(reg.projects.is_empty());
}

#[test]
fn unuse_rejects_the_whole_call_when_one_target_is_not_registered() {
    let (_install, paths) = setup_with_skill(None);
    let global_dir = tempfile::tempdir().unwrap();
    let agent = global_agent(&paths, global_dir.path());
    let project = temp_project();
    use_skill(
        paths.root(),
        "demo",
        None,
        &[agent],
        Targets::global(),
        false,
    )
    .unwrap();

    // 项目没有登记：整次不动盘，全局落点与记录都留着
    let err = unuse(
        paths.root(),
        "demo",
        Targets::new(true, vec![project.clone()]),
        None,
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "not_registered");
    assert!(global_dir.path().join("demo").exists(), "整次不动盘");
    assert_eq!(hub::load_registry(&paths).unwrap().global().skills.len(), 1);
}
