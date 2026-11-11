//! 技能状态模块直测：存在性判据。
//! 只经公开操作摆出盘面（入库、归档），再直接查状态。

use super::*;
use crate::skill::state::{SkillState, skill_status};

#[test]
fn active_skill_has_workdir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例");
    add(&root(&install), &src, None, false).unwrap();

    let status = skill_status(&paths, "demo");
    assert_eq!(status.state(), SkillState::Active);
    assert!(status.is_present());
}

#[test]
fn archived_skill_has_no_workdir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "gone", None, "归档");
    add(&root(&install), &src, None, false).unwrap();
    archive(&root(&install), "gone", false).unwrap();

    let status = skill_status(&paths, "gone");
    assert_eq!(status.state(), SkillState::Archived);
    assert!(status.is_present());
}

#[test]
fn missing_skill_reports_not_found() {
    let (_install, paths) = setup();
    let status = skill_status(&paths, "ghost");
    assert_eq!(status.state(), SkillState::Missing);
    assert!(matches!(
        status.require_present(),
        Err(crate::skill::state::StateError::SkillNotFound { .. })
    ));
    assert!(matches!(
        status.require_active(),
        Err(crate::skill::state::StateError::SkillNotFound { .. })
    ));
}

#[test]
fn active_skill_passes_requirement_checks() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "fresh", None, "活跃");
    add(&root(&install), &src, None, false).unwrap();
    let fresh = skill_status(&paths, "fresh");
    assert!(fresh.require_active().is_ok());
    assert!(fresh.require_present().is_ok());
}
