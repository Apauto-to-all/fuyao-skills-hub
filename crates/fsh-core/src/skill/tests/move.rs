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
    add(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", None, "演示"),
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
    archive(&root(&install), "a/b/tdd", false).unwrap();
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
    add(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", None, "演示"),
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

/// 真·中途失败回滚：两个落点、第二个重链时父目录被普通文件挡死（建链失败），
/// 回滚清单逆序执行——store 目录挪回原位、首落点重建旧链、登记簿整文件还原
#[test]
fn move_midway_failure_rolls_back_every_touched_step() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", None, "演示"),
        Some("a"),
        false,
    )
    .unwrap();
    // 两个共用同一项目技能目录的 agent → 两条记录、两个不同落点各一条
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert!(
        crate::project::use_skill(
            &root(&install),
            "a/tdd",
            &["universal".to_string(), "dsh".to_string()],
            crate::project::Targets::new(false, vec![project.path().to_path_buf()]),
            false,
        )
        .is_ok(),
        "use 夹具应成功"
    );
    let first_dest = project.path().join(".agents/skills/tdd");
    let second_dest = project.path().join(".dsh/skills/tdd");
    let reg_before = fs::read_to_string(paths.projects_toml()).unwrap();
    let second_parent = second_dest.parent().unwrap().to_path_buf();

    // 注入失败：第二个落点的父目录换成一个普通文件——重链在 create_dir_all
    // 处报 io 错误；remove_link 删叶子不碰父目录，注入在前不挡预检与首落点
    fs::rename(&second_parent, project.path().join("skills-moved-away")).unwrap();
    fs::write(&second_parent, "挡路").unwrap();

    let result = move_skill(&root(&install), "a/tdd", "tdd", false);
    assert!(result.is_err(), "第二个落点建链要失败");
    // 还原注入物，让断言读得到盘面
    fs::remove_file(&second_parent).unwrap();
    fs::rename(project.path().join("skills-moved-away"), &second_parent).unwrap();

    // 回滚断言：store 目录回原位、登记簿整文件还原
    assert!(
        paths.skill_workdir("a/tdd").join("SKILL.md").is_file(),
        "store 目录回原位"
    );
    assert!(!paths.skill_workdir("tdd").exists(), "新键不得留盘");
    assert_eq!(
        fs::read_to_string(paths.projects_toml()).unwrap(),
        reg_before,
        "登记簿整文件还原"
    );
    // 首落点回到旧链：形态是链接且穿透到旧 store 路径
    let meta = first_dest.symlink_metadata().unwrap();
    assert!(
        meta.file_type().is_symlink(),
        "首落点应重建为链接（junction 表现为 reparse point）"
    );
    assert!(
        fs::canonicalize(&first_dest).unwrap()
            == fs::canonicalize(paths.skill_workdir("a/tdd")).unwrap(),
        "首落点穿透后指旧 store 路径"
    );
    let _ = proj;
}
