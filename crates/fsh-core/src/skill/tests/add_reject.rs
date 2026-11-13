//! 入库拒绝矩阵：非法来源、重复、目录名不符、前缀形态、
//! 键与既有内容冲突、坏入参先于 hub 状态的报错序。

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
        add(&root(&install), &empty, None, false),
        Err(SkillError::InvalidSource { .. })
    ));
    // SKILL.md 无 frontmatter
    let no_fm = src_dir.path().join("no-fm");
    fs::create_dir_all(&no_fm).unwrap();
    fs::write(no_fm.join("SKILL.md"), "# 只有正文\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_fm, None, false),
        Err(SkillError::InvalidSource { .. })
    ));
    // frontmatter 无 name
    let no_name = src_dir.path().join("no-name");
    fs::create_dir_all(&no_name).unwrap();
    fs::write(no_name.join("SKILL.md"), "---\ndescription: 无名\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_name, None, false),
        Err(SkillError::InvalidSource { .. })
    ));
}

/// 未初始化的 hub 上坏入参先报入参错误：入库的纯入参判定
/// （源可读、name 与目录一致、键形状）不依赖 hub，不落到「hub 未初始化」
#[test]
fn add_reports_bad_input_before_hub_state() {
    let bare = tempfile::tempdir().unwrap();
    let hub = bare.path();
    let src_dir = tempfile::tempdir().unwrap();
    // 无 SKILL.md
    let empty = src_dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    // name 与目录名不一致
    let mismatch = src_dir.path().join("dir-x");
    fs::create_dir_all(&mismatch).unwrap();
    fs::write(
        mismatch.join("SKILL.md"),
        "---\nname: other\ndescription: 名不对\n---\n",
    )
    .unwrap();
    // name 形状非法
    let bad_name = src_dir.path().join("bad");
    fs::create_dir_all(&bad_name).unwrap();
    fs::write(
        bad_name.join("SKILL.md"),
        "---\nname: a\\\\b\ndescription: 坏名\n---\n",
    )
    .unwrap();
    for (label, src, prefix) in [
        ("无 SKILL.md", &empty, None),
        ("name 与目录不一致", &mismatch, None),
        ("name 形状非法", &bad_name, None),
        ("前缀形状非法", &mismatch, Some("C:/x")),
    ] {
        let err = add(hub, src, prefix, false).unwrap_err();
        assert_ne!(
            err.error_code(),
            "hub_not_initialized",
            "{label} 应先报入参错误"
        );
        assert!(
            matches!(err, SkillError::InvalidSource { .. })
                || err.error_code() == "invalid_skill_key",
            "{label} 应报入参错误，实际 {err:?}"
        );
    }
    // 预演同序：不落盘的路上坏入参同样先报
    let err = add(hub, &empty, None, true).unwrap_err();
    assert_ne!(err.error_code(), "hub_not_initialized");
    assert!(matches!(err, SkillError::InvalidSource { .. }));
}

#[test]
fn add_rejects_duplicate_by_workdir() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "dup", None, "重复");
    add(&root(&install), &src, None, false).unwrap();
    // 工作副本占位 → 拒绝
    assert!(matches!(
        add(&root(&install), &src, None, false),
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
        add(&root(&install), &dir, None, false),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn add_rejects_malformed_prefix() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "ok", None, "本身合法");
    // 形状不对的键前缀由 key 校验器拒绝
    for bad in [
        "bad\\prefix",
        "a//b",
        "/lead",
        "tail/",
        "..",
        "a/./b",
        "C:/x",
    ] {
        assert!(
            matches!(
                add(&root(&install), &src, Some(bad), false),
                Err(SkillError::Common(crate::error::CommonError::InvalidKey(_)))
            ),
            "非法前缀应被拒绝: {bad}"
        );
    }
    // 纯空白前缀经规范化后等价空串，按无前缀处理：合法
    assert!(add(&root(&install), &src, Some(""), false).is_ok());
}

#[test]
fn add_rejects_key_inside_existing_skill_content() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let parent = make_skill(src_dir.path(), "tdd", None, "父键");
    add(&root(&install), &parent, None, false).unwrap();
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
            add(&root(&install), &nested, Some("tdd"), false),
            Err(SkillError::InvalidSource { .. })
        ),
        "键落在已有 skill 内容树里应被拒绝"
    );
}

#[test]
fn add_rejects_reusing_existing_prefix_layer() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let nested = make_skill(src_dir.path(), "tdd", None, "先入嵌套键");
    add(&root(&install), &nested, Some("mattpocock/skills"), false).unwrap();
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
            add(&root(&install), &flat, Some("mattpocock"), false),
            Err(SkillError::AlreadyExists { .. })
        ),
        "键目录已被前缀层占用应被拒绝"
    );
}
