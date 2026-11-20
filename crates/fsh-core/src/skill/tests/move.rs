//! 键迁移测试：四闸拒绝的先后、根级目标键与中途失败回滚。
//! 断言只经公开入口 move_skill：预演走四闸并回动作清单，实跑回两端键。

use std::fs;

use super::*;
use crate::error::ErrorCode;
use crate::plan::ActionKind;

/// 四闸的拒绝各自可触发且互不遮蔽：末段不等、来源缺席（归档态单列）、
/// 目标占用、归档面同名占用。四闸全过才有动作清单，清单首步恒为挪目录
#[test]
fn move_gates_reject_in_turn() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", "演示"),
        Some("a/b"),
        false,
    )
    .unwrap();

    // 闸一：末段不等
    let err = move_skill(&root(&install), "a/b/tdd", "c/d/other", true).unwrap_err();
    assert_eq!(err.error_code(), "identity_immutable");
    assert!(err.to_string().contains("重新 add"), "要指路重入库: {err}");

    // 闸二：旧键不在 store（完全不存在）
    let err = move_skill(&root(&install), "a/b/nope", "c/b/nope", true).unwrap_err();
    assert_eq!(err.error_code(), "skill_not_found");

    // 闸二：旧键只在归档面
    archive_key(&root(&install), "a/b/tdd", false).unwrap();
    let err = move_skill(&root(&install), "a/b/tdd", "c/b/tdd", true).unwrap_err();
    assert_eq!(err.error_code(), "archived");
    restore(&root(&install), "a/b/tdd", false).unwrap();

    // 闸三：新键 store 路径被占（手造一个目录占住落点）
    fs::create_dir_all(paths.skill_workdir("c/b/tdd")).unwrap();
    let err = move_skill(&root(&install), "a/b/tdd", "c/b/tdd", true).unwrap_err();
    assert_eq!(err.error_code(), "already_exists");
    fs::remove_dir_all(paths.skill_workdir("c/b/tdd")).unwrap();

    // 闸四：新键不在归档面占用
    fs::create_dir_all(paths.skill_archived("c/b/tdd")).unwrap();
    let err = move_skill(&root(&install), "a/b/tdd", "c/b/tdd", true).unwrap_err();
    assert_eq!(err.error_code(), "conflict");
    fs::remove_dir_all(paths.skill_archived("c/b/tdd")).unwrap();

    // 四闸全过：清单首步是挪目录，末步是提交
    let dry = move_skill(&root(&install), "a/b/tdd", "c/b/tdd", true).unwrap();
    assert!(dry.dry_run, "预演信封带 dry_run");
    assert_eq!(dry.from, "a/b/tdd");
    assert_eq!(dry.actions.first().unwrap().action, ActionKind::Move);
    assert!(
        matches!(dry.actions.last().unwrap().action, ActionKind::Commit),
        "末步是提交"
    );
    // 预演零落盘：store 两侧都不动
    assert!(paths.skill_workdir("a/b/tdd").join("SKILL.md").is_file());
    assert!(!paths.skill_workdir("c/b/tdd").exists());
}

/// 根级目标键（摘掉全部前缀）过四闸：末段就是键本身
#[test]
fn move_allows_root_level_target() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", "演示"),
        Some("a"),
        false,
    )
    .unwrap();

    let applied = move_skill(&root(&install), "a/tdd", "tdd", false).unwrap();
    assert_eq!(applied.to, "tdd");
    assert!(
        paths.skill_workdir("tdd").join("SKILL.md").is_file(),
        "工作副本落到根级键"
    );
    assert!(!paths.skill_workdir("a/tdd").exists(), "旧位置清空");
}

/// 登记写的是 copy，落点就必须还是真目录：重链不挑降级链上更省事的一级，
/// 于是迁移后的落点仍与登记同档，撤回不会撞上「落点与登记不符」
#[test]
fn move_keeps_a_copy_destination_a_real_directory() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", "演示"),
        Some("a"),
        false,
    )
    .unwrap();
    // 手造 copy 档的现场：登记写 copy，落点是真目录（拷贝不跟源同步）
    let project = tempfile::tempdir().unwrap();
    let dest = project.path().join(".agents/skills/tdd");
    fs::create_dir_all(&dest).unwrap();
    fs::write(
        dest.join("SKILL.md"),
        "---\nname: tdd\ndescription: 手改快照\n---\n",
    )
    .unwrap();
    let mut registry = crate::hub::ProjectRegistry::default();
    registry.record_use(
        &crate::project::Anchor::Project(project.path().to_path_buf()),
        crate::hub::ProjectSkill {
            key: "a/tdd".into(),
            agents: vec!["universal".to_string()],
            link: crate::link::LinkKind::Copy,
        },
        1,
    );
    crate::hub::save_registry(&paths, &registry, &crate::lock::write_lock(&paths).unwrap())
        .unwrap();

    move_skill(&root(&install), "a/tdd", "b/tdd", false).unwrap();

    let meta = dest.symlink_metadata().unwrap();
    assert!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "copy 落点不得升成链接"
    );
    assert_eq!(
        fs::read_to_string(dest.join("SKILL.md")).unwrap(),
        "---\nname: tdd\ndescription: 手改快照\n---\n",
        "拷贝内容原样，不随源刷新"
    );
    // 撤回照登记的档位撤：形态与登记一致，不会报 link_kind_mismatch
    crate::project::unuse(
        &root(&install),
        &crate::select::Selector::named(Some("b".to_string()), vec!["tdd".to_string()]),
        crate::project::Targets::new(false, vec![project.path().to_path_buf()]),
        &["universal".to_string()],
        true,
        false,
    )
    .unwrap();
    assert!(!dest.exists(), "撤回删净拷贝落点");
}

/// 真·中途失败回滚：两个落点各一条链，落盘最后一步写登记簿时被挡死——
/// 原子写的临时文件位置被一个目录占住，登记簿写不出去。回滚先让 store 目录
/// 回位，再逐落点删新链、按旧源重建旧链。登记簿在这次失败里
/// 从未被打开写，字节与修改时间都停在开工前
#[test]
fn move_midway_failure_rolls_back_every_touched_step() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", "演示"),
        Some("a"),
        false,
    )
    .unwrap();
    // 两个共用同一项目技能目录的 agent → 两条记录、两个不同落点各一条
    let project = tempfile::tempdir().unwrap();
    crate::project::use_skill(
        &root(&install),
        &crate::select::Selector::named(Some("a".to_string()), vec!["tdd".to_string()]),
        &["universal".to_string(), "dsh".to_string()],
        crate::project::Targets::new(false, vec![project.path().to_path_buf()]),
        false,
    )
    .expect("use 夹具应成功");
    // 落点档位由本机降级链定下（symlink → junction → copy），回滚按同一档位重建
    let kind = crate::hub::load_registry(&paths)
        .unwrap()
        .records()
        .next()
        .expect("use 后登记簿应有一条记录")
        .skill()
        .link;
    let first_dest = project.path().join(".agents/skills/tdd");
    let second_dest = project.path().join(".dsh/skills/tdd");
    let reg_before = fs::read_to_string(paths.projects_toml()).unwrap();
    let reg_mtime_before = fs::metadata(paths.projects_toml())
        .unwrap()
        .modified()
        .unwrap();
    // 注入失败：登记簿原子写的临时文件位置换成目录，写不出去
    let staging = paths.projects_toml().with_extension("toml.tmp");
    fs::create_dir(&staging).unwrap();

    let result = move_skill(&root(&install), "a/tdd", "tdd", false);
    let err = result.expect_err("登记簿写不出去，整笔迁移要失败");
    assert_eq!(err.error_code(), "io", "失败点落在登记簿写回: {err}");
    // 撤掉注入物，让断言读得到盘面
    fs::remove_dir(&staging).unwrap();

    // 回滚断言：store 目录回原位，登记簿连一个字节都没被写过
    assert!(
        paths.skill_workdir("a/tdd").join("SKILL.md").is_file(),
        "store 目录回原位"
    );
    assert!(!paths.skill_workdir("tdd").exists(), "新键不得留盘");
    assert_eq!(
        fs::read_to_string(paths.projects_toml()).unwrap(),
        reg_before,
        "登记簿字节不变"
    );
    assert_eq!(
        fs::metadata(paths.projects_toml())
            .unwrap()
            .modified()
            .unwrap(),
        reg_mtime_before,
        "登记簿未被重写：修改时间停在开工前"
    );
    assert!(
        !paths.projects_toml().with_extension("toml.tmp").exists(),
        "失败后不留登记簿临时文件"
    );
    // 两个落点各自回到旧链：形态随本机档位（symlink / junction / copy），
    // 契约只有一条——落点在、可用，且穿透到已回位的旧工作副本
    for dest in [&first_dest, &second_dest] {
        assert!(dest.exists(), "回滚后落点应在场: {}", dest.display());
        match kind {
            // copy 是快照：内容照旧，不随源刷新
            crate::link::LinkKind::Copy => assert_eq!(
                fs::read_to_string(dest.join("SKILL.md")).unwrap(),
                fs::read_to_string(paths.skill_workdir("a/tdd").join("SKILL.md")).unwrap(),
                "拷贝落点内容与旧工作副本一致: {}",
                dest.display()
            ),
            // 链接与 junction 都穿透到旧 store 路径（junction 是 reparse point，
            // is_symlink 为假，故只认穿透结果）
            crate::link::LinkKind::Symlink | crate::link::LinkKind::Junction => assert_eq!(
                fs::canonicalize(dest).unwrap(),
                fs::canonicalize(paths.skill_workdir("a/tdd")).unwrap(),
                "落点穿透后指旧 store 路径: {}",
                dest.display()
            ),
        }
    }
}
