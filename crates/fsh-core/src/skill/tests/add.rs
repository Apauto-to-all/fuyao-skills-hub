//! 入库测试：建工作副本、版本来源、嵌套键建层、前端字段要求。

use std::fs;
use std::path::Path;

use super::*;

#[test]
fn add_creates_workdir_and_machine_commit() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例技能");

    let report = add(&root(&install), &src, None, false).unwrap();
    assert_eq!(
        report,
        SkillOutcome {
            key: "demo".into(),
            ..SkillOutcome::default()
        }
    );
    // 工作副本落盘：SKILL.md 就是元数据唯一住所
    assert!(paths.skill_workdir("demo").join("SKILL.md").is_file());
    // 源内容逐字节保留：frontmatter 原样落盘
    let work = fs::read_to_string(paths.skill_workdir("demo").join("SKILL.md")).unwrap();
    assert!(work.contains("name: demo"), "frontmatter 原样透传: {work}");
    assert!(!paths.root().join("meta").exists(), "meta 目录不应存在");
    // 入库产生机器提交、工作区干净
    let repo = git2::Repository::open(paths.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.message().unwrap(), "add(store): demo");
    assert_eq!(commit.committer().name().unwrap(), "fsh");
    assert!(!git::is_dirty(&repo).unwrap());
}

#[test]
fn add_excludes_nested_git_dir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "nested", None, "嵌套仓库");
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
    let src = make_skill(src_dir.path(), "tdd", None, "嵌套键");
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
fn add_requires_description_but_not_source() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let no_desc = src_dir.path().join("no-desc");
    fs::create_dir_all(&no_desc).unwrap();
    fs::write(no_desc.join("SKILL.md"), "---\nname: no-desc\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_desc, None, false),
        Err(SkillError::InvalidSource { .. })
    ));
    // 自产 skill 不写 source 也能入库
    let own = make_skill(src_dir.path(), "own", None, "自产");
    assert_eq!(add(&root(&install), &own, None, false).unwrap().key, "own");
}
