//! 技能状态模块直测：存在性判据。
//! 只经公开操作摆出盘面（入库、归档），再直接查状态。

use super::*;
use crate::skill::state::{SkillState, skill_status};

/// 打开 hub 仓库的只读句柄（测试观察用）
fn open_repo(paths: &HubPaths) -> git2::Repository {
    git2::Repository::open(paths.root()).unwrap()
}

#[test]
fn active_skill_has_workdir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例");
    add(&root(&install), &src, None, false).unwrap();

    let repo = open_repo(&paths);
    let status = skill_status(&repo, &paths, "demo").unwrap();
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

    let repo = open_repo(&paths);
    let status = skill_status(&repo, &paths, "gone").unwrap();
    assert_eq!(status.state(), SkillState::Archived);
    assert!(status.is_present());
}

#[test]
fn missing_skill_reports_not_found() {
    let (_install, paths) = setup();
    let repo = open_repo(&paths);
    let status = skill_status(&repo, &paths, "ghost").unwrap();
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
    let repo = open_repo(&paths);
    let fresh = skill_status(&repo, &paths, "fresh").unwrap();
    assert!(fresh.require_active().is_ok());
    assert!(fresh.require_present().is_ok());
}
