//! 多目标撤回测试：目标集合的合法性、全局与项目同一次调用、一个键的目标不全时整键跳过。

use super::*;
use crate::hub;

#[test]
fn unuse_without_any_target_is_rejected() {
    let (_install, paths) = setup_with_skill();
    let err = unuse(
        paths.root(),
        &demo_selector(),
        Targets::default(),
        &[],
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "no_target");
}

#[test]
fn unuse_rejects_a_project_that_does_not_exist() {
    let (_install, paths) = setup_with_skill();
    let missing = tempfile::tempdir().unwrap().path().join("gone");
    let err = unuse(
        paths.root(),
        &demo_selector(),
        Targets::new(false, vec![missing.clone()]),
        &[],
        false,
        false,
    )
    .unwrap_err();
    // 目录不存在是目标本身不合格，不该伪装成「没有登记记录」
    assert_eq!(err.error_code(), "invalid_project");
}

#[test]
fn unuse_global_removes_global_destinations_and_record() {
    let (_install, paths) = setup_with_skill();
    let global_dir = tempfile::tempdir().unwrap();
    let agent = global_agent(&paths, global_dir.path());
    use_skill(
        paths.root(),
        "demo",
        std::slice::from_ref(&agent),
        Targets::new(true, Vec::new()),
        false,
    )
    .unwrap();
    assert!(global_dir.path().join("demo").exists());

    let r = unuse(
        paths.root(),
        &demo_selector(),
        Targets::new(true, Vec::new()),
        std::slice::from_ref(&agent),
        false,
        false,
    )
    .unwrap();
    assert_eq!(r.applied, vec!["demo".to_string()]);
    assert!(r.skipped.is_empty());
    assert!(!global_dir.path().join("demo").exists());
    let reg = hub::load_registry(&paths).unwrap();
    assert!(reg.global().skills.is_empty(), "全局记录应删除");
}

#[test]
fn unuse_serves_global_and_projects_in_one_call_without_touching_others() {
    let (_install, paths) = setup_with_skill();
    let global_dir = tempfile::tempdir().unwrap();
    // 两侧都能用的 agent：项目位落在项目里，全局位落在临时目录（不碰真实家目录）。
    // 一次调用的名单要同时命中全部目标，只有一个 agent 时两处记录的名单才可能一致
    fs::create_dir_all(paths.agents()).unwrap();
    fs::write(
        paths.agents().join("both.toml"),
        format!(
            "skills_dir = \".both/skills\"\nglobal_skills_dir = {:?}\n",
            global_dir.path().display().to_string()
        ),
    )
    .unwrap();
    let agent = vec!["both".to_string()];
    let first = temp_project();
    let second = temp_project();
    // 全局与两个项目各用一次 use：两处都铺给同一个 agent
    use_skill(
        paths.root(),
        "demo",
        &agent,
        Targets::new(true, Vec::new()),
        false,
    )
    .unwrap();
    use_skill(
        paths.root(),
        "demo",
        &agent,
        Targets::new(false, vec![first.clone(), second.clone()]),
        false,
    )
    .unwrap();

    // 只撤第二个项目：全局与第一个项目一字不动
    unuse(
        paths.root(),
        &demo_selector(),
        Targets::new(false, vec![second.as_path().into()]),
        &agent,
        false,
        false,
    )
    .unwrap();
    assert!(global_dir.path().join("demo").exists(), "全局落点不得被撤");
    assert!(
        first.join(".both/skills/demo").exists(),
        "第一个项目不得被撤"
    );
    assert!(record(&paths, first.as_path(), "demo").is_some());
    assert!(record(&paths, second.as_path(), "demo").is_none());
    assert_eq!(hub::load_registry(&paths).unwrap().global().skills.len(), 1);

    // 全局与第一个项目一起撤：一次调用撤净两处
    let r = unuse(
        paths.root(),
        &demo_selector(),
        Targets::new(true, vec![first.clone()]),
        &agent,
        false,
        false,
    )
    .unwrap();
    assert_eq!(r.applied, vec!["demo".to_string()]);
    assert!(!global_dir.path().join("demo").exists());
    assert!(!first.join(".both/skills/demo").exists());
    let reg = hub::load_registry(&paths).unwrap();
    assert!(reg.global().skills.is_empty());
    assert!(reg.project_dirs().next().is_none());
}

#[test]
fn unuse_skips_a_skill_when_one_target_is_not_registered() {
    let (_install, paths) = setup_with_skill();
    let global_dir = tempfile::tempdir().unwrap();
    let agent = global_agent(&paths, global_dir.path());
    let project = temp_project();
    use_skill(
        paths.root(),
        "demo",
        &[agent],
        Targets::new(true, Vec::new()),
        false,
    )
    .unwrap();

    // 项目没有登记：这个键整键跳过，全局落点与记录都留着
    let r = unuse(
        paths.root(),
        &demo_selector(),
        Targets::new(true, vec![project.clone()]),
        &[],
        false,
        false,
    )
    .unwrap();
    assert!(r.applied.is_empty());
    assert_eq!(r.skipped[0].code, "not_registered");
    assert_eq!(r.code(), 1, "一条都没做成时退出码为 1");
    assert!(global_dir.path().join("demo").exists(), "整键不动盘");
    assert_eq!(hub::load_registry(&paths).unwrap().global().skills.len(), 1);
}

#[test]
fn unuse_skips_a_skill_when_one_target_lacks_the_agent() {
    let (_install, paths) = setup_with_skill();
    let global_dir = tempfile::tempdir().unwrap();
    let global_only = global_agent(&paths, global_dir.path());
    let project = temp_project();
    // 两条记录的 agent 名单不同：全局只有全局位的 agent，项目用通用位
    use_skill(
        paths.root(),
        "demo",
        std::slice::from_ref(&global_only),
        Targets::new(true, Vec::new()),
        false,
    )
    .unwrap();
    use_skill(
        paths.root(),
        "demo",
        &["universal".to_string()],
        Targets::new(false, vec![project.clone()]),
        false,
    )
    .unwrap();

    // 名单里的 agent 只在全局登记：这个键整键跳过，两处都不动
    let r = unuse(
        paths.root(),
        &demo_selector(),
        Targets::new(true, vec![project.clone()]),
        std::slice::from_ref(&global_only),
        false,
        false,
    )
    .unwrap();
    assert!(r.applied.is_empty());
    assert_eq!(r.skipped[0].code, "agent_not_registered");
    assert_eq!(r.code(), 1, "一条都没做成时退出码为 1");
    assert!(
        global_dir.path().join("demo").exists(),
        "先查后动：全局落点不得被撤"
    );
    assert!(project.join(".agents/skills/demo").exists());
    assert_eq!(
        hub::load_registry(&paths).unwrap().global().skills[0].agents,
        vec![global_only],
        "全局记录一字不动"
    );
    assert!(record(&paths, &project, "demo").is_some());
}
