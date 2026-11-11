//! 使用测试：落点建立与登记、预检拒绝、撤回后按固定版本重铺、未初始化拒绝。

use std::fs;

use super::*;

#[test]
fn use_latest_links_workdir_and_registers_once() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = two_agents();
    let r = use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();

    assert_eq!(r.targets[0].agents, agents);
    assert_eq!(r.targets[0].destinations.len(), 2);
    // 两个不同落点都真的能读到正文
    for name in [".agents/skills/demo", ".dsh/skills/demo"] {
        let p = project.as_path().join(name);
        assert!(p.join("SKILL.md").is_file(), "{name} 应可读");
        assert!(p.join("references/a.txt").is_file());
    }
    let rec = record(&paths, project.as_path(), "demo").expect("登记簿应有记录");
    assert_eq!(rec.agents, agents);
    assert_eq!(rec.link, r.targets[0].link.expect("实跑必须给出链接方式"));
}

#[test]
fn use_rejects_unknown_skill_version_and_missing_project() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = ["universal".to_string()];
    let err = use_skill(
        paths.root(),
        "ghost",
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "skill_not_found");
    let missing = project.as_path().join("no-such-dir");
    let err = use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![missing.clone()]),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "invalid_project");
}

#[test]
fn use_rejects_duplicate_registration_and_occupied_destinations() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();

    // 已登记：必须先 unuse
    let err = use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "already_registered");

    // 另一个项目里同名落点已被用户占用：拒绝且不覆盖
    let other = temp_project();
    let occupied = other.as_path().join(".agents/skills/demo");
    fs::create_dir_all(&occupied).unwrap();
    fs::write(occupied.join("user.txt"), "keep").unwrap();
    let err = use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![other.as_path().into()]),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "destination_occupied");
    assert_eq!(
        fs::read_to_string(occupied.join("user.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn use_dedupes_agents_sharing_one_destination() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    // 通用位与 codex 共用 .agents/skills
    let agents = vec!["universal".to_string(), "codex".to_string()];
    let r = use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert_eq!(r.targets[0].destinations.len(), 1, "共用落点只建一次");
    assert_eq!(r.targets[0].destinations[0].agents, agents);
    let rec = record(&paths, project.as_path(), "demo").unwrap();
    assert_eq!(rec.agents, agents);
}

/// 使用通常不弄脏 hub 仓库（落点在项目侧、登记簿 ignored）：
/// 自动提交按脏检查静默跳过，不产生空提交
#[test]
fn use_skips_commit_when_hub_workdir_clean() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head_before = repo.head().unwrap().peel_to_commit().unwrap().id();
    use_skill(
        paths.root(),
        "demo",
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert_eq!(
        repo.head().unwrap().peel_to_commit().unwrap().id(),
        head_before,
        "无变更不得产生空提交"
    );
    assert!(!git::is_dirty(&repo).unwrap());
}

/// 撤回后可重新使用：落点重建、记录重新登记
#[test]
fn use_after_unuse_repaves() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    unuse(
        paths.root(),
        "demo",
        Targets::new(false, vec![project.as_path().into()]),
        None,
        false,
        false,
    )
    .unwrap();
    let r = use_skill(
        paths.root(),
        "demo",
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert_eq!(r.targets[0].destinations.len(), 1);
    assert!(record(&paths, project.as_path(), "demo").is_some());
}

/// 未初始化 hub：项目使用必须先报 hub_not_initialized
#[test]
fn use_requires_initialized_hub() {
    let dir = tempfile::tempdir().unwrap();
    let project = temp_project();
    let err = use_skill(
        dir.path(),
        "demo",
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "hub_not_initialized");
}

/// 多个目标建完落点后登记簿写不进去：本次已建入口必须回滚干净，不留没有记录的落点
#[test]
fn registry_write_failure_rolls_back_created_destinations() {
    let (_install, paths) = setup_with_skill(None);
    let first = temp_project();
    let second = temp_project();
    // 登记簿的临时文件位置被目录占住：落点建完之后写盘才失败
    fs::create_dir_all(paths.projects_toml().with_extension("toml.tmp")).unwrap();
    let err = use_skill(
        paths.root(),
        "demo",
        &["universal".to_string()],
        Targets::new(false, vec![first.clone(), second.clone()]),
        false,
    )
    .unwrap_err();

    assert_eq!(err.error_code(), "io");
    for project in [&first, &second] {
        assert!(
            !project.join(".agents/skills/demo").exists(),
            "回滚后不得残留入口: {}",
            project.display()
        );
    }
}
