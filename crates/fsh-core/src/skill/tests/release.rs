//! 发版测试：脏改动收集、拒绝非法发版、透传字段保留、版本必须前进。

use std::fs;

use super::*;

#[test]
fn release_collects_dirty_changes_into_new_version() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "grow", None, "成长"),
        None,
    )
    .unwrap();
    // 工作副本改脏（编辑面随便改是常态），号写在文件里
    fs::write(
        paths.skill_workdir("grow").join("SKILL.md"),
        "---\nname: grow\ndescription: 成长 v2\nversion: \"0.2.0\"\n---\n\n新版正文\n",
    )
    .unwrap();
    let next = release(&root(&install), "grow").unwrap();
    assert_eq!(next, "0.2.0");
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "grow").unwrap(),
        vec!["0.2.0", "0.1.0"]
    );
    assert!(!git::is_dirty(&repo).unwrap(), "脏改动应被收编进新版本");
    // 新 tag 的 SKILL.md 内容 = 改后版本
    let text = skill_md_from_tag(&repo, "grow/v0.2.0", "grow")
        .unwrap()
        .unwrap();
    assert!(text.contains("新版正文"));
}

#[test]
fn release_rejects_archived_or_missing() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "gone", None, "归档"),
        None,
    )
    .unwrap();
    archive(&root(&install), "gone").unwrap();
    assert!(matches!(
        release(&root(&install), "gone"),
        Err(SkillError::Archived { .. })
    ));
    assert!(matches!(
        release(&root(&install), "never"),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn passthrough_fields_survive_release() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("pass");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("SKILL.md"),
        "---\nname: pass\ndescription: 透传\nlicense: MIT\ncompatibility: claude-code\nallowed-tools: [Bash, Read]\n---\n\n正文\n",
    )
    .unwrap();
    add(&root(&install), &src, None).unwrap();
    // 只改号发新版：写回只碰 version 行，其余字段与正文透传
    set_version(&paths, "pass", "0.1.1");
    release(&root(&install), "pass").unwrap();
    let after = fs::read_to_string(paths.skill_workdir("pass").join("SKILL.md")).unwrap();
    assert!(
        after.contains("version: \"0.1.1\""),
        "版本号应留在编辑面: {after}"
    );
    for kept in [
        "license: MIT",
        "compatibility: claude-code",
        "allowed-tools: [Bash, Read]",
        "---\n\n正文",
    ] {
        assert!(after.contains(kept), "透传不丢: {kept}");
    }
    // 版本树里的 SKILL.md 与编辑面逐字节一致
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        skill_md_from_tag(&repo, "pass/v0.1.1", "pass")
            .unwrap()
            .unwrap(),
        after
    );
}

#[test]
fn version_from_frontmatter_must_advance() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "adv", None, "递增"),
        None,
    )
    .unwrap();
    let work = paths.skill_workdir("adv").join("SKILL.md");
    let repo = git2::Repository::open(paths.root()).unwrap();

    // 手工抹掉号 → 号由文件定，没得发
    fs::write(&work, "---\nname: adv\ndescription: 递增\n---\n").unwrap();
    assert!(matches!(
        release(&root(&install), "adv"),
        Err(SkillError::InvalidSource { .. })
    ));
    // 与最新 tag 齐平或倒退 → 拒绝并提示改大
    for stale in ["0.1.0", "0.0.9"] {
        fs::write(
            &work,
            format!("---\nname: adv\ndescription: 递增\nversion: \"{stale}\"\n---\n"),
        )
        .unwrap();
        assert!(matches!(
            release(&root(&install), "adv"),
            Err(SkillError::VersionNotAdvancing { .. })
        ));
    }
    // 文件号领先 → 发版成功，tag = 文件号
    fs::write(
        &work,
        "---\nname: adv\ndescription: 递增\nversion: \"1.0.0\"\n---\n",
    )
    .unwrap();
    assert_eq!(release(&root(&install), "adv").unwrap(), "1.0.0");
    assert_eq!(git::tag_list(&repo, "adv").unwrap(), vec!["1.0.0", "0.1.0"]);
}
