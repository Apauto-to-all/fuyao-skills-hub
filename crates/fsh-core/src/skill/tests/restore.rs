//! 恢复测试：从归档版本重建工作副本且不产生新标签。

use std::fs;

use super::*;
use crate::skill::frontmatter::parse;

#[test]
fn restore_recovers_workdir_without_new_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "back", None, "复活");
    add(&root(&install), &src, None, false).unwrap();
    // 编辑面（工作副本）改出 v2 内容后发版，归档，再恢复
    fs::write(
        paths.skill_workdir("back").join("SKILL.md"),
        "---\nname: back\ndescription: 复活 v2\nmetadata:\n  version: \"1.0.0\"\n---\n\nv2 正文\n",
    )
    .unwrap();
    release(&root(&install), "back", false).unwrap();
    archive(&root(&install), "back", false).unwrap();
    restore(&root(&install), "back", false).unwrap();
    // 工作副本按最新 tag 内容复活：frontmatter 全字段原样回来
    let text = fs::read_to_string(paths.skill_workdir("back").join("SKILL.md")).unwrap();
    assert!(text.contains("v2 正文"));
    let fm = parse(&text, "SKILL.md").unwrap();
    assert_eq!(fm.name, "back");
    assert_eq!(fm.description, "复活 v2");
    assert_eq!(fm.version.as_deref(), Some("1.0.0"));
    // 版本链原样（无新 tag）、工作区干净
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "back").unwrap(),
        vec!["1.0.0", "0.1.0"]
    );
    assert!(!git::is_dirty(&repo).unwrap());
}

#[test]
fn restore_rejects_active_or_unknown() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "live", None, "活跃"),
        None,
        false,
    )
    .unwrap();
    assert!(matches!(
        restore(&root(&install), "live", false),
        Err(SkillError::AlreadyExists { .. })
    ));
    assert!(matches!(
        restore(&root(&install), "ghost", false),
        Err(SkillError::SkillNotFound { .. })
    ));
}
