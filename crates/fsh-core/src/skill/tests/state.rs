//! 技能状态模块直测：两个占位与各操作前置。
//! 只经公开操作摆出盘面（入库、归档），再直接取占位与前置判定。

use super::*;
use crate::skill::state::{ActiveError, StateError, skill_occupancy};

#[test]
fn active_skill_occupies_workdir_only() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", "示例");
    add(&root(&install), &src, None, false).unwrap();

    let workdir = paths.skill_workdir("demo");
    let occupancy = skill_occupancy(&paths, "demo");
    assert_eq!(occupancy.workdir(), Some(workdir.as_path()));
    assert!(occupancy.archive().is_none(), "归档侧没有副本");
    assert_eq!(
        occupancy.require_active().unwrap(),
        workdir,
        "活跃技能给出工作副本作为来源"
    );
    assert!(matches!(
        occupancy.require_vacant(),
        Err(StateError::AlreadyExists { .. })
    ));
    assert!(occupancy.require_removable().is_ok());
}

#[test]
fn archived_skill_occupies_archive_only() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "gone", "归档"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "gone", false).unwrap();

    let archived = paths.skill_archived("gone");
    let occupancy = skill_occupancy(&paths, "gone");
    assert!(occupancy.workdir().is_none(), "store 侧没有工作副本");
    assert_eq!(occupancy.archive(), Some(archived.as_path()));
    assert!(matches!(
        occupancy.require_active(),
        Err(ActiveError::Archived { .. })
    ));
    assert_eq!(
        occupancy.require_restorable().unwrap().from,
        archived,
        "恢复来源是归档副本"
    );
    assert!(occupancy.require_removable().is_ok());
}

#[test]
fn missing_skill_has_no_occupancy_and_rejects_every_operation() {
    let (_install, paths) = setup();
    let occupancy = skill_occupancy(&paths, "ghost");
    assert!(occupancy.workdir().is_none());
    assert!(occupancy.archive().is_none());
    assert!(matches!(
        occupancy.require_active(),
        Err(ActiveError::SkillNotFound { .. })
    ));
    for refused in [
        occupancy.require_restorable().map(|_| ()),
        occupancy.require_removable(),
    ] {
        assert!(matches!(refused, Err(StateError::SkillNotFound { .. })));
    }
    assert!(occupancy.require_vacant().is_ok(), "两侧都空才允许入库");
}

/// 两个面各自独立：两侧都有副本时两个事实都成立，目标面占用照实拒绝
#[test]
fn both_copies_are_two_independent_facts() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "twin", "两侧都有"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "twin", false).unwrap();
    let work = paths.skill_workdir("twin");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    let occupancy = skill_occupancy(&paths, "twin");
    assert!(occupancy.workdir().is_some(), "工作副本在");
    assert!(occupancy.archive().is_some(), "归档副本也在");
    assert!(matches!(
        occupancy.require_free_archive(),
        Err(StateError::Conflict { .. })
    ));
    assert!(matches!(
        occupancy.require_restorable(),
        Err(StateError::Conflict { .. })
    ));
    assert!(occupancy.require_removable().is_ok());
}

/// 判据统一为目录存在：同名普通文件不算副本
#[test]
fn plain_files_are_not_copies() {
    let (_install, paths) = setup();
    fs::create_dir_all(paths.store()).unwrap();
    fs::create_dir_all(paths.archive()).unwrap();
    fs::write(paths.skill_workdir("plain"), "不是目录").unwrap();
    fs::write(paths.skill_archived("plain"), "不是目录").unwrap();

    let occupancy = skill_occupancy(&paths, "plain");
    assert!(occupancy.workdir().is_none(), "判据只看目录");
    assert!(occupancy.archive().is_none(), "判据只看目录");
}

/// 活跃技能没有归档副本：恢复的目标面已被工作副本占住，报 conflict
#[test]
fn restore_rejects_active_skill_because_store_side_is_taken() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "demo", "活跃"),
        None,
        false,
    )
    .unwrap();
    assert!(matches!(
        skill_occupancy(&paths, "demo").require_restorable(),
        Err(StateError::Conflict { .. })
    ));
}
