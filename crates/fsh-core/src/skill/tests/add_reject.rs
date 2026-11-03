//! 入库拒绝矩阵：非法来源、重复、目录名不符、前缀形态、键与既有内容冲突。

use std::fs;

use super::*;

#[test]
fn add_rejects_invalid_sources() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    // 无 SKILL.md
    let empty = src_dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        add(&root(&install), &empty, None),
        Err(SkillError::InvalidSource { .. })
    ));
    // SKILL.md 无 frontmatter
    let no_fm = src_dir.path().join("no-fm");
    fs::create_dir_all(&no_fm).unwrap();
    fs::write(no_fm.join("SKILL.md"), "# 只有正文\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_fm, None),
        Err(SkillError::InvalidSource { .. })
    ));
    // frontmatter 无 name
    let no_name = src_dir.path().join("no-name");
    fs::create_dir_all(&no_name).unwrap();
    fs::write(no_name.join("SKILL.md"), "---\ndescription: 无名\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_name, None),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn add_rejects_duplicate_by_workdir_or_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "dup", None, "重复");
    add(&root(&install), &src, None).unwrap();
    // 工作副本占位 → 拒绝
    assert!(matches!(
        add(&root(&install), &src, None),
        Err(SkillError::AlreadyExists { .. })
    ));
    // 归档后（无工作副本、有 tag）→ 仍然拒绝
    archive(&root(&install), "dup").unwrap();
    assert!(!paths.skill_workdir("dup").exists());
    assert!(matches!(
        add(&root(&install), &src, None),
        Err(SkillError::AlreadyExists { .. })
    ));
}

#[test]
fn add_rejects_name_directory_mismatch() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let dir = src_dir.path().join("dir-x");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: other\ndescription: 名不对\n---\n",
    )
    .unwrap();
    assert!(matches!(
        add(&root(&install), &dir, None),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn add_rejects_malformed_prefix() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "ok", None, "本身合法");
    // 纯空白前缀经规范化后等价空串，按无前缀处理；其余非法形态拒绝
    for bad in [
        "bad\\prefix",
        "a//b",
        "/lead",
        "tail/",
        "..",
        "a/./b",
        "C:/x",
        "1.2.3",
    ] {
        assert!(
            matches!(
                add(&root(&install), &src, Some(bad)),
                Err(SkillError::InvalidSource { .. })
            ),
            "非法前缀应被拒绝: {bad}"
        );
    }
    // 空串等价于无前缀：合法
    assert!(add(&root(&install), &src, Some("")).is_ok());
}

#[test]
fn add_rejects_key_inside_existing_skill_content() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let parent = make_skill(src_dir.path(), "pdf", None, "父键");
    add(&root(&install), &parent, None).unwrap();
    // 父键已是 skill：嵌套其下会被拒（两棵版本树会重叠）
    let nested = src_dir.path().join("inner");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join("SKILL.md"),
        "---\nname: inner\ndescription: 嵌套\n---\n",
    )
    .unwrap();
    assert!(
        matches!(
            add(&root(&install), &nested, Some("pdf")),
            Err(SkillError::InvalidSource { .. })
        ),
        "键落在已有 skill 内容树里应被拒绝"
    );
}

#[test]
fn add_rejects_reusing_existing_prefix_layer() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let nested = make_skill(src_dir.path(), "pdf", None, "先入嵌套键");
    add(&root(&install), &nested, Some("anthropics/skills")).unwrap();
    // 前缀层目录已被嵌套键占用：同名的扁平键不得再落进来
    let flat = src_dir.path().join("skills");
    fs::create_dir_all(&flat).unwrap();
    fs::write(
        flat.join("SKILL.md"),
        "---\nname: skills\ndescription: 撞前缀层\n---\n",
    )
    .unwrap();
    assert!(
        matches!(
            add(&root(&install), &flat, Some("anthropics")),
            Err(SkillError::AlreadyExists { .. })
        ),
        "键目录已被前缀层占用应被拒绝"
    );
}

#[test]
fn add_rejects_version_like_name() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let dir = src_dir.path().join("1.2.3");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: 1.2.3\ndescription: 形如版本目录\n---\n",
    )
    .unwrap();
    assert!(
        matches!(
            add(&root(&install), &dir, None),
            Err(SkillError::InvalidSource { .. })
        ),
        "目录名形如版本目录应被拒绝"
    );
}
