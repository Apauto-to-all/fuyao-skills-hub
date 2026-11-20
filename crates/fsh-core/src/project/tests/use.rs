//! 使用测试：落点建立与登记、预检跳过、撤回后重新铺装、未初始化拒绝。

use std::fs;

use super::*;

/// 落点被普通文件占着：跳过并报告，码不变，说得出占着的是什么。
/// 两处占用来源都要带得出种类：落点自身是文件、父目录链上有同名文件
#[test]
fn occupied_destination_is_skipped_with_what_occupies_it() {
    let agents = ["universal".to_string()];

    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    let occupied = project.join(".agents/skills/demo");
    fs::create_dir_all(occupied.parent().unwrap()).unwrap();
    fs::write(&occupied, "用户文件").unwrap();
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project]),
        false,
    )
    .unwrap();
    assert!(r.applied.is_empty(), "跳过的技能不进 applied");
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].code, "destination_occupied");
    assert!(
        r.skipped[0].message.contains("占着的是普通文件"),
        "应说出占着的是普通文件: {}",
        r.skipped[0].message
    );

    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    fs::create_dir_all(project.join(".agents")).unwrap();
    fs::write(project.join(".agents/skills"), "挡路的文件").unwrap();
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project]),
        false,
    )
    .unwrap();
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].code, "destination_occupied");
    assert!(
        r.skipped[0].message.contains("占着的是普通文件"),
        "父目录链上的文件也应点名种类: {}",
        r.skipped[0].message
    );
}

/// 两个目标各建各的落点、各记一条记录：链接闸按目标收口，
/// 一个目标落到什么档位不影响另一个目标
#[test]
fn use_two_targets_each_record_their_own_link() {
    let (_install, paths) = setup_with_skill();
    let first = temp_project();
    let second = temp_project();
    let agents = ["universal".to_string()];
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![first.clone(), second.clone()]),
        false,
    )
    .unwrap();

    assert_eq!(r.applied, ["demo"]);
    for project in [&first, &second] {
        assert!(project.join(".agents/skills/demo/SKILL.md").is_file());
        let rec = record(&paths, project, "demo").expect("每个目标各有一条记录");
        assert_eq!(rec.agents, agents);
        assert!(
            matches!(
                rec.link,
                crate::link::LinkKind::Symlink
                    | crate::link::LinkKind::Junction
                    | crate::link::LinkKind::Copy
            ),
            "记录必须带得出实际档位"
        );
    }
}

#[test]
fn use_links_workdir_and_registers_once() {
    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    let agents = two_agents();
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();

    // 两个 agent 各一处落点，同一个技能只算一条 applied、一条登记记录
    assert_eq!(r.applied, ["demo"]);
    assert!(r.skipped.is_empty());
    // 两个不同落点都真的能读到正文
    for name in [".agents/skills/demo", ".dsh/skills/demo"] {
        let p = project.as_path().join(name);
        assert!(p.join("SKILL.md").is_file(), "{name} 应可读");
        assert!(p.join("references/a.txt").is_file());
    }
    let rec = record(&paths, project.as_path(), "demo").expect("登记簿应有记录");
    assert_eq!(rec.agents, agents);
}

/// 预演清单的顺序即执行顺序：全部落点建完才写登记簿
#[test]
fn dry_run_lists_registers_after_every_link() {
    let (_install, paths) = setup_with_skill();
    let first = temp_project();
    let second = temp_project();
    let agents = two_agents();
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![first.as_path().into(), second.as_path().into()]),
        true,
    )
    .unwrap();

    let steps = r.actions;
    let last_link = steps
        .iter()
        .rposition(|step| step.action == crate::plan::ActionKind::Link)
        .expect("预演应列出链接步");
    let first_register = steps
        .iter()
        .position(|step| step.action == crate::plan::ActionKind::Register)
        .expect("预演应列出登记步");
    assert!(
        last_link < first_register,
        "登记必须排在全部落点之后: {steps:?}"
    );
    assert_eq!(
        steps
            .iter()
            .filter(|step| step.action == crate::plan::ActionKind::Register)
            .count(),
        2,
        "每个目标各一条登记"
    );
}

#[test]
fn use_rejects_unknown_skill_and_missing_project() {
    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    let err = use_skill(
        paths.root(),
        &selector(&["ghost"]),
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "no_match");
    let missing = project.as_path().join("no-such-dir");
    let err = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &["universal".to_string()],
        Targets::new(false, vec![missing]),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "invalid_project");
}

/// 已登记或落点被占：预期内的不适用，跳过并报告，不覆盖已有内容
#[test]
fn use_skips_duplicate_registration_and_occupied_destinations() {
    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();

    // 已登记：跳过并报 already_registered，落点仍是原来那一处
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert!(r.applied.is_empty());
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].code, "already_registered");

    // 另一个项目里同名落点已被用户占用：跳过且不覆盖
    let other = temp_project();
    let occupied = other.as_path().join(".agents/skills/demo");
    fs::create_dir_all(&occupied).unwrap();
    fs::write(occupied.join("user.txt"), "keep").unwrap();
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![other.as_path().into()]),
        false,
    )
    .unwrap();
    assert!(r.applied.is_empty());
    assert_eq!(r.skipped[0].code, "destination_occupied");
    assert_eq!(
        fs::read_to_string(occupied.join("user.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn use_dedupes_agents_sharing_one_destination() {
    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    // 通用位与 codex 共用 .agents/skills
    let agents = vec!["universal".to_string(), "codex".to_string()];
    let dry = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        true,
    )
    .unwrap();
    assert_eq!(
        dry.actions
            .iter()
            .filter(|step| step.action == crate::plan::ActionKind::Link)
            .count(),
        1,
        "共用落点只建一次"
    );
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert_eq!(r.applied, ["demo"]);
    let rec = record(&paths, project.as_path(), "demo").unwrap();
    assert_eq!(rec.agents, agents, "共用落点的 agent 合并进同一条记录");
}

/// 使用通常不弄脏 hub 仓库（落点在项目侧、登记簿 ignored）：
/// 自动提交按脏检查静默跳过，不产生空提交
#[test]
fn use_skips_commit_when_hub_workdir_clean() {
    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head_before = repo.head().unwrap().peel_to_commit().unwrap().id();
    use_skill(
        paths.root(),
        &selector(&["demo"]),
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
    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    unuse(
        paths.root(),
        &selector(&["demo"]),
        Targets::new(false, vec![project.as_path().into()]),
        &agents,
        false,
        false,
    )
    .unwrap();
    let r = use_skill(
        paths.root(),
        &selector(&["demo"]),
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert_eq!(r.applied, ["demo"]);
    assert!(project.join(".agents/skills/demo").is_dir());
    assert!(record(&paths, project.as_path(), "demo").is_some());
}

/// 未初始化 hub：项目使用必须先报 hub_not_initialized
#[test]
fn use_requires_initialized_hub() {
    let dir = tempfile::tempdir().unwrap();
    let project = temp_project();
    let err = use_skill(
        dir.path(),
        &selector(&["demo"]),
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
    let (_install, paths) = setup_with_skill();
    let first = temp_project();
    let second = temp_project();
    // 登记簿的临时文件位置被目录占住：落点建完之后写盘才失败
    fs::create_dir_all(paths.projects_toml().with_extension("toml.tmp")).unwrap();
    let err = use_skill(
        paths.root(),
        &selector(&["demo"]),
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

/// 同一套里两个技能末段重名：后者与前者争同一处落点，跳过并报告，不整批失败
#[test]
fn same_destination_in_one_batch_skips_the_later_key() {
    let (_install, paths) = setup_with_skill();
    // 再入一个 b/demo：落点名同样是 demo，与根下的 demo 争同一处
    let src_root = tempfile::tempdir().unwrap();
    let src = src_root.path().join("demo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    crate::skill::add(paths.root(), &src, Some("b"), false).unwrap();

    let project = temp_project();
    let r = use_skill(
        paths.root(),
        &Selector::all(None),
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert!(
        r.applied.contains(&"b/demo".to_string()),
        "按选择顺序先到的那个照常铺: {:?}",
        r.applied
    );
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].key, "demo");
    assert_eq!(r.skipped[0].code, "destination_occupied");
}
