//! 技能状态模块直测：三态判据、版本清单与无 tag 的活跃形态。
//! 只经公开操作摆出盘面（入库、发版、归档、手删 tag），再直接查状态。

use super::*;
use crate::skill::state::{SkillState, skill_status};

/// 打开 hub 仓库的只读句柄（测试观察用）
fn open_repo(paths: &HubPaths) -> git2::Repository {
    git2::Repository::open(paths.root()).unwrap()
}

#[test]
fn active_skill_has_workdir_and_version_list() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例");
    add(&root(&install), &src, None, false).unwrap();
    // 发一版后再看：版本清单按新在前排列，最新版本即表头
    set_version(&paths, "demo", "0.2.0");
    release(
        &root(&install),
        "demo",
        ReleaseSource::Frontmatter,
        false,
        false,
    )
    .unwrap();

    let repo = open_repo(&paths);
    let status = skill_status(&repo, &paths, "demo").unwrap();
    assert_eq!(status.state(), SkillState::Active);
    assert_eq!(
        status.versions().collect::<Vec<_>>(),
        vec!["0.2.0", "0.1.0"]
    );
    assert_eq!(status.latest(), Some("0.2.0"));
    assert_eq!(status.key(), "demo");
}

#[test]
fn archived_skill_has_versions_but_no_workdir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "gone", None, "归档");
    add(&root(&install), &src, None, false).unwrap();
    archive(&root(&install), "gone", false).unwrap();

    let repo = open_repo(&paths);
    let status = skill_status(&repo, &paths, "gone").unwrap();
    assert_eq!(status.state(), SkillState::Archived);
    assert_eq!(status.versions().collect::<Vec<_>>(), vec!["0.1.0"]);
    assert_eq!(status.latest(), Some("0.1.0"));
}

#[test]
fn missing_skill_has_neither_workdir_nor_versions() {
    let (_install, paths) = setup();
    let repo = open_repo(&paths);
    let status = skill_status(&repo, &paths, "ghost").unwrap();
    assert_eq!(status.state(), SkillState::Missing);
    assert_eq!(status.versions().collect::<Vec<_>>(), Vec::<&str>::new());
    assert_eq!(status.latest(), None);
}

#[test]
fn active_without_tags_stays_active_with_empty_versions() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "bare", None, "无版本链");
    add(&root(&install), &src, None, false).unwrap();
    // 手删首版 tag：工作副本仍在的键保持活跃，版本清单为空
    let repo = open_repo(&paths);
    let tag = crate::naming::version_tag("bare", "0.1.0");
    repo.tag_delete(&tag).unwrap();

    let status = skill_status(&repo, &paths, "bare").unwrap();
    assert_eq!(status.state(), SkillState::Active);
    assert_eq!(status.versions().collect::<Vec<_>>(), Vec::<&str>::new());
    assert_eq!(status.latest(), None);
    assert!(!status.has_versions());
}

#[test]
fn requirement_checks_map_states_to_three_errors() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "held", None, "被查");
    add(&root(&install), &src, None, false).unwrap();
    archive(&root(&install), "held", false).unwrap();

    let repo = open_repo(&paths);
    // 不存在的键：在场与活跃两种要求都按不存在拒绝
    let missing = skill_status(&repo, &paths, "ghost").unwrap();
    assert!(matches!(
        missing.require_present(),
        Err(crate::skill::state::StateError::SkillNotFound { .. })
    ));
    assert!(matches!(
        missing.require_active(),
        Err(crate::skill::state::StateError::SkillNotFound { .. })
    ));
    // 归档键：在场放行、活跃拒绝
    let archived = skill_status(&repo, &paths, "held").unwrap();
    assert!(archived.require_present().is_ok());
    assert!(matches!(
        archived.require_active(),
        Err(crate::skill::state::StateError::Archived { .. })
    ));
    // 活跃键：在场且活跃放行；版本不在清单里按 version_not_found 拒绝
    let src2 = make_skill(src_dir.path(), "fresh", None, "活跃");
    add(&root(&install), &src2, None, false).unwrap();
    let fresh = skill_status(&repo, &paths, "fresh").unwrap();
    assert!(fresh.require_active().is_ok());
    assert!(fresh.require_present().is_ok());
    assert!(fresh.require_version("0.1.0").is_ok());
    assert!(matches!(
        fresh.require_version("9.9.9"),
        Err(crate::skill::state::StateError::VersionNotFound { .. })
    ));
}
