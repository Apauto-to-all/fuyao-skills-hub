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
        false,
    )
    .unwrap();
    // 工作副本改脏（编辑面随便改是常态），号写在文件里
    fs::write(
        paths.skill_workdir("grow").join("SKILL.md"),
        "---\nname: grow\ndescription: 成长 v2\nmetadata:\n  version: \"0.2.0\"\n---\n\n新版正文\n",
    )
    .unwrap();
    let next = release(
        &root(&install),
        "grow",
        ReleaseSource::Frontmatter,
        false,
        false,
    )
    .unwrap()
    .version;
    assert_eq!(next, "0.2.0");
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "grow").unwrap(),
        vec!["0.2.0", "0.1.0"]
    );
    assert!(!git::is_dirty(&repo).unwrap(), "脏改动应被收编进新版本");
    // 新 tag 的 SKILL.md 内容 = 改后版本
    let text = git::read_blob_at(&repo, "grow/v0.2.0", "store/grow/SKILL.md")
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
        false,
    )
    .unwrap();
    archive(&root(&install), "gone", false).unwrap();
    assert!(matches!(
        release(
            &root(&install),
            "gone",
            ReleaseSource::Frontmatter,
            false,
            false
        ),
        Err(SkillError::Archived { .. })
    ));
    assert!(matches!(
        release(
            &root(&install),
            "never",
            ReleaseSource::Frontmatter,
            false,
            false
        ),
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
    add(&root(&install), &src, None, false).unwrap();
    // 只改号发新版：写回只碰 version 行，其余字段与正文透传
    set_version(&paths, "pass", "0.1.1");
    release(
        &root(&install),
        "pass",
        ReleaseSource::Frontmatter,
        false,
        false,
    )
    .unwrap();
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
        git::read_blob_at(&repo, "pass/v0.1.1", "store/pass/SKILL.md")
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
        false,
    )
    .unwrap();
    let work = paths.skill_workdir("adv").join("SKILL.md");
    let repo = git2::Repository::open(paths.root()).unwrap();

    // 手工抹掉号 → 号由文件定，没得发
    fs::write(&work, "---\nname: adv\ndescription: 递增\n---\n").unwrap();
    assert!(matches!(
        release(
            &root(&install),
            "adv",
            ReleaseSource::Frontmatter,
            false,
            false
        ),
        Err(SkillError::InvalidSource { .. })
    ));
    // 与最新 tag 齐平或倒退 → 拒绝并提示改大
    for stale in ["0.1.0", "0.0.9"] {
        fs::write(
            &work,
            format!("---\nname: adv\ndescription: 递增\nmetadata:\n  version: \"{stale}\"\n---\n"),
        )
        .unwrap();
        assert!(matches!(
            release(
                &root(&install),
                "adv",
                ReleaseSource::Frontmatter,
                false,
                false
            ),
            Err(SkillError::VersionNotAdvancing { .. })
        ));
    }
    // 文件号领先 → 发版成功，tag = 文件号
    fs::write(
        &work,
        "---\nname: adv\ndescription: 递增\nmetadata:\n  version: \"1.0.0\"\n---\n",
    )
    .unwrap();
    assert_eq!(
        release(
            &root(&install),
            "adv",
            ReleaseSource::Frontmatter,
            false,
            false
        )
        .unwrap()
        .version,
        "1.0.0"
    );
    assert_eq!(git::tag_list(&repo, "adv").unwrap(), vec!["1.0.0", "0.1.0"]);
}

#[test]
fn release_explicit_writes_typed_version_then_publishes() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "typed", None, "手输版本"),
        None,
        false,
    )
    .unwrap();

    // frontmatter 里没有版本号，界面输入 0.3.0 后应写回文件并发出同号 tag
    let next = release(
        &root(&install),
        "typed",
        ReleaseSource::Explicit("0.3.0"),
        false,
        false,
    )
    .unwrap()
    .version;
    assert_eq!(next, "0.3.0");
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "typed").unwrap(),
        vec!["0.3.0", "0.1.0"],
        "入库时的首版仍在，新号排在前"
    );
    let text = fs::read_to_string(paths.skill_workdir("typed").join("SKILL.md")).unwrap();
    assert!(text.contains("version: \"0.3.0\""), "号应写回工作副本");
}

#[test]
fn release_explicit_dry_run_lists_actions_and_writes_nothing() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "glance", None, "显式预演"),
        None,
        false,
    )
    .unwrap();

    // 显式号预演：写回、提交、打标逐项列出，盘上原封不动
    let before = fs::read_to_string(paths.skill_workdir("glance").join("SKILL.md")).unwrap();
    let outcome = release(
        &root(&install),
        "glance",
        ReleaseSource::Explicit("0.2.0"),
        false,
        true,
    )
    .unwrap();
    assert!(outcome.dry_run);
    assert_eq!(
        outcome.actions,
        vec![
            "写回 metadata.version = 0.2.0".to_string(),
            "commit: release glance v0.2.0".to_string(),
            "tag glance/v0.2.0".to_string(),
        ]
    );
    // frontmatter 与入库时逐字节一致，也没有新 tag
    let after = fs::read_to_string(paths.skill_workdir("glance").join("SKILL.md")).unwrap();
    assert_eq!(after, before, "预演不应写回版本号");
    assert_eq!(
        git::tag_list(&git2::Repository::open(paths.root()).unwrap(), "glance").unwrap(),
        vec!["0.1.0"]
    );
}

#[test]
fn release_explicit_rejects_existing_version_without_overwrite() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "same", Some("0.1.0"), "同号"),
        None,
        false,
    )
    .unwrap();

    assert!(matches!(
        release(
            &root(&install),
            "same",
            ReleaseSource::Explicit("0.1.0"),
            false,
            false,
        ),
        Err(SkillError::VersionNotAdvancing { .. })
    ));
}

#[test]
fn release_explicit_overwrite_moves_latest_tag_to_new_commit() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "redo", Some("0.1.0"), "重发"),
        None,
        false,
    )
    .unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    let before = git::resolve(&repo, "redo/v0.1.0").unwrap().0;

    // 工作副本改脏后同号重发：tag 必须指向新提交，版本数不变
    fs::write(
        paths.skill_workdir("redo").join("SKILL.md"),
        "---\nname: redo\ndescription: 重发第二稿\nmetadata:\n  version: \"0.1.0\"\n---\n\n新正文\n",
    )
    .unwrap();
    release(
        &root(&install),
        "redo",
        ReleaseSource::Explicit("0.1.0"),
        true,
        false,
    )
    .unwrap();
    let after = git::resolve(&repo, "redo/v0.1.0").unwrap().0;
    assert_ne!(before, after, "覆盖应把 tag 移到新提交");
    assert_eq!(git::tag_list(&repo, "redo").unwrap(), vec!["0.1.0"]);
    let text = git::read_blob_at(&repo, "redo/v0.1.0", "store/redo/SKILL.md")
        .unwrap()
        .unwrap();
    assert!(text.contains("新正文"));
}

#[test]
fn release_explicit_rejects_older_version_even_with_overwrite() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "old", Some("0.1.0"), "旧号");
    add(&root(&install), &src, None, false).unwrap();
    fs::write(
        _paths.skill_workdir("old").join("SKILL.md"),
        "---\nname: old\ndescription: 旧号\nmetadata:\n  version: \"0.2.0\"\n---\n\n正文\n",
    )
    .unwrap();
    release(
        &root(&install),
        "old",
        ReleaseSource::Frontmatter,
        false,
        false,
    )
    .unwrap();

    assert!(matches!(
        release(
            &root(&install),
            "old",
            ReleaseSource::Explicit("0.1.0"),
            true,
            false,
        ),
        Err(SkillError::VersionNotAdvancing { .. })
    ));
}

#[test]
fn release_explicit_rejects_malformed_version() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "bad", Some("0.1.0"), "坏号"),
        None,
        false,
    )
    .unwrap();

    assert!(matches!(
        release(
            &root(&install),
            "bad",
            ReleaseSource::Explicit("1.x"),
            false,
            false,
        ),
        Err(SkillError::InvalidVersion { .. })
    ));
}

#[test]
fn release_explicit_rejects_two_segment_version() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "short", Some("0.1.0"), "两段号"),
        None,
        false,
    )
    .unwrap();

    // 两段号能通过宽松字符检查，但写进 frontmatter 会让 parse 与 tag 反解都失配
    assert!(matches!(
        release(
            &root(&install),
            "short",
            ReleaseSource::Explicit("1.2"),
            false,
            false,
        ),
        Err(SkillError::InvalidVersion { .. })
    ));
}
