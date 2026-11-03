//! 入库测试：建工作副本与标签、版本来源、嵌套键建层、前端字段要求。

use std::fs;

use super::*;

#[test]
fn add_creates_workdir_and_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例技能");

    let report = add(&root(&install), &src, None).unwrap();
    assert_eq!(
        report,
        AddReport {
            key: "demo".into(),
            version: "0.1.0".into()
        }
    );
    // 工作副本落盘：SKILL.md 就是元数据唯一住所
    assert!(paths.skill_workdir("demo").join("SKILL.md").is_file());
    // 缺省的号要落回编辑面：号只有一个诞生地（文件）
    let work = fs::read_to_string(paths.skill_workdir("demo").join("SKILL.md")).unwrap();
    assert!(
        work.contains("version: \"0.1.0\""),
        "缺省首版号应写回: {work}"
    );
    assert!(!paths.root().join("meta").exists(), "meta 目录不应存在");
    // 首版 tag 就位；入库不产导出目录，导出由 use 按需生成
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "demo").unwrap(), vec!["0.1.0"]);
    assert!(!paths.exported("demo", "0.1.0").exists());
    // 提交完成、工作区干净
    assert!(!git::is_dirty(&repo).unwrap());
}

#[test]
fn add_honors_frontmatter_version() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "ver-skill", Some("2.1.0"), "带版本");
    let report = add(&root(&install), &src, None).unwrap();
    assert_eq!(report.version, "2.1.0");
}

#[test]
fn add_excludes_nested_git_dir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "nested", None, "嵌套仓库");
    fs::create_dir_all(src.join(".git").join("objects")).unwrap();
    fs::write(src.join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    add(&root(&install), &src, None).unwrap();
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
    let src = make_skill(src_dir.path(), "pdf", None, "嵌套键");
    let report = add(&root(&install), &src, Some("anthropics/skills")).unwrap();
    assert_eq!(report.key, "anthropics/skills/pdf");
    // 工作副本与 tag 全部按嵌套键就位
    assert!(
        paths
            .skill_workdir("anthropics/skills/pdf")
            .join("SKILL.md")
            .is_file()
    );
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "anthropics/skills/pdf").unwrap(),
        vec!["0.1.0"]
    );
    assert!(!git::is_dirty(&repo).unwrap());
    // 忽略规则深度无关：导出目录一律 ignored（无论键多深），工作副本始终 tracked
    assert!(
        repo.status_should_ignore(Path::new("export/anthropics/skills/pdf/0.1.0/SKILL.md"))
            .unwrap(),
        "多级键的导出路径应被忽略"
    );
    assert!(
        !repo
            .status_should_ignore(Path::new("store/anthropics/skills/pdf/SKILL.md"))
            .unwrap(),
        "工作副本应 tracked"
    );
}

#[test]
fn add_requires_description_and_quoted_version_but_not_source() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let no_desc = src_dir.path().join("no-desc");
    fs::create_dir_all(&no_desc).unwrap();
    fs::write(no_desc.join("SKILL.md"), "---\nname: no-desc\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_desc, None),
        Err(SkillError::InvalidSource { .. })
    ));
    let bare = src_dir.path().join("bare");
    fs::create_dir_all(&bare).unwrap();
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: bare\ndescription: 裸号\nversion: 1.2.0\n---\n",
    )
    .unwrap();
    assert!(matches!(
        add(&root(&install), &bare, None),
        Err(SkillError::InvalidSource { .. })
    ));
    // 自产 skill 不写 source 也能入库，缺省 version 取首版 0.1.0
    let own = make_skill(src_dir.path(), "own", None, "自产");
    assert_eq!(add(&root(&install), &own, None).unwrap().version, "0.1.0");
}
