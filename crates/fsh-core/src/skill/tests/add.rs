//! 入库测试：建工作副本、嵌套键建层、description 必填。

use std::fs;
use std::path::Path;

use super::*;

#[test]
fn add_excludes_nested_git_dir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "nested", "嵌套仓库");
    fs::create_dir_all(src.join(".git").join("objects")).unwrap();
    fs::write(src.join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    add(&root(&install), &src, None, false).unwrap();
    assert!(!paths.skill_workdir("nested").join(".git").exists());
    assert!(
        paths
            .skill_workdir("nested")
            .join("references")
            .join("a.txt")
            .is_file()
    );
}

#[test]
fn add_with_prefix_creates_nested_key() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "tdd", "嵌套键");
    let report = add(&root(&install), &src, Some("mattpocock/skills"), false).unwrap();
    assert_eq!(report.key, "mattpocock/skills/tdd");
    // 工作副本按嵌套键就位
    assert!(
        paths
            .skill_workdir("mattpocock/skills/tdd")
            .join("SKILL.md")
            .is_file()
    );
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(!git::is_dirty(&repo).unwrap());
    // 工作副本始终 tracked
    assert!(
        !repo
            .status_should_ignore(Path::new("store/mattpocock/skills/tdd/SKILL.md"))
            .unwrap(),
        "工作副本应 tracked"
    );
}

#[test]
fn add_requires_description() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let no_desc = src_dir.path().join("no-desc");
    fs::create_dir_all(&no_desc).unwrap();
    fs::write(no_desc.join("SKILL.md"), "---\nname: no-desc\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_desc, None, false),
        Err(SkillError::InvalidSource { .. })
    ));
}
