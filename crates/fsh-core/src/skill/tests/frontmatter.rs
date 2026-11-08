//! frontmatter 测试：metadata 子块里的五字段解析与写回、顶层错位的拒绝、
//! identity 两字段的规范约束、metadata 形状本身的约束。

use super::*;
use crate::skill::frontmatter::{parse, set_metadata_version, unquote};

/// metadata 子块是 fsh 自有三字段的唯一住所；顶层只留官方字段，
/// 其余顶层字段不解析也不影响结果
#[test]
fn frontmatter_reads_five_fields_from_metadata_block() {
    let text = "---\nname: tdd\ndescription: 测试驱动开发\nlicense: MIT\nmetadata:\n  version: \"1.2.0\"\n  source: mattpocock/skills\n  tags: \"tdd, office\"\n  author: someone\n---\n\n正文\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.name, "tdd");
    assert_eq!(fm.description, "测试驱动开发");
    assert_eq!(fm.version.as_deref(), Some("1.2.0"));
    assert_eq!(fm.source.as_deref(), Some("mattpocock/skills"));
    assert_eq!(fm.tags, vec!["tdd", "office"]);
}

/// tags 是一个逗号分隔的字符串：按逗号拆、去首尾空格、丢空项
#[test]
fn tags_split_on_comma_and_drop_blank_items() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  tags: \" tdd ,, office ,\"\n---\n";
    assert_eq!(parse(text, "SKILL.md").unwrap().tags, vec!["tdd", "office"]);
}

/// 子键的缩进深度不影响读取：整块按块内最小缩进去缩进
#[test]
fn metadata_sub_keys_read_at_any_uniform_indent() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n    version: \"1.0.0\"\n    tags: \"a, b\"\n---\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.version.as_deref(), Some("1.0.0"));
    assert_eq!(fm.tags, vec!["a", "b"]);
}

/// metadata 块里的空行只是排版：空行之后写的子键照样读得到
#[test]
fn sub_key_after_blank_line_inside_metadata_is_read() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  source: owner/repo\n\n  version: \"1.2.0\"\n---\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.source.as_deref(), Some("owner/repo"));
    assert_eq!(fm.version.as_deref(), Some("1.2.0"));
}

/// metadata 里只放 fsh 不认识的键时，三个自有字段全缺省
#[test]
fn metadata_without_fsh_keys_yields_empty_optionals() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  author: someone\n---\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.version, None);
    assert_eq!(fm.source, None);
    assert!(fm.tags.is_empty());
}

/// metadata 整块缺省同样合法：只有 name / description 必填
#[test]
fn missing_metadata_block_is_allowed() {
    let fm = parse("---\nname: tdd\ndescription: 描述\n---\n", "SKILL.md").unwrap();
    assert_eq!(fm.name, "tdd");
    assert_eq!(fm.version, None);
    assert!(fm.tags.is_empty());
}

/// 顶层出现 fsh 自有字段即错位置：错误点名该字段与正确位置
#[test]
fn rejects_fsh_fields_at_top_level() {
    for key in ["version", "source", "tags"] {
        let text = format!("---\nname: tdd\ndescription: 描述\n{key}: x\n---\n");
        let err = parse(&text, "SKILL.md").unwrap_err();
        let SkillError::InvalidSource { reason, .. } = &err else {
            panic!("顶层 {key} 应报无效源: {err:?}");
        };
        assert!(
            reason.contains(key) && reason.contains("metadata"),
            "文案要点名字段与正确位置: {reason}"
        );
    }
}

/// metadata.version 的规则：必须带引号的三段数字
#[test]
fn rejects_metadata_version_that_is_unquoted_or_not_semver() {
    for version in ["1.2", "1.2.0", "v1.2.0", "\"1.2\"", "\"v1.2.0\""] {
        let text =
            format!("---\nname: tdd\ndescription: 描述\nmetadata:\n  version: {version}\n---\n");
        assert!(
            matches!(
                parse(&text, "SKILL.md"),
                Err(SkillError::InvalidSource { .. })
            ),
            "非法 metadata.version 应被拒绝: {version}"
        );
    }
}

/// metadata.tags 只能是逗号分隔的字符串：流式与块式列表都被拒绝
#[test]
fn rejects_yaml_list_tags_under_metadata() {
    for tags in ["[tdd, office]", "[tdd]", "\n    - tdd\n    - office"] {
        let text = format!("---\nname: tdd\ndescription: 描述\nmetadata:\n  tags:{tags}\n---\n");
        let err = parse(&text, "SKILL.md").unwrap_err();
        let SkillError::InvalidSource { reason, .. } = &err else {
            panic!("列表式 tags 应报无效源: {err:?}");
        };
        assert!(
            reason.contains("tags") && reason.contains("字符串"),
            "文案要点名 tags 与正确写法: {reason}"
        );
    }
}

/// metadata 只能是缩进块：行内值会让子键读不到，一律拒绝而不是静默当空
#[test]
fn rejects_inline_metadata_that_is_not_a_block() {
    for value in ["{version: \"1.0.0\"}", "[a]", "\"\""] {
        let text = format!("---\nname: tdd\ndescription: 描述\nmetadata: {value}\n---\n");
        assert!(
            matches!(
                parse(&text, "SKILL.md"),
                Err(SkillError::InvalidSource { .. })
            ),
            "行内 metadata 应被拒绝: {value}"
        );
    }
}

/// name 按规范收紧：字符集、首尾与连续连字符、长度上限
#[test]
fn rejects_name_outside_spec_charset_or_length() {
    for name in [
        "Tdd",
        "td_d",
        "中文名",
        "-tdd",
        "tdd-",
        "td--d",
        "tdd.skill",
        &"a".repeat(65),
    ] {
        let text = format!("---\nname: {name}\ndescription: 描述\n---\n");
        assert!(
            matches!(
                parse(&text, "SKILL.md"),
                Err(SkillError::InvalidSource { .. })
            ),
            "非法 name 应被拒绝: {name}"
        );
    }
    // 规范上限内的名字与描述照常通过
    let text = format!(
        "---\nname: {}\ndescription: {}\n---\n",
        "a".repeat(64),
        "字".repeat(1024)
    );
    assert!(parse(&text, "SKILL.md").is_ok(), "上限以内应通过");
}

/// description 非空且不超过 1024 字符（按 Unicode 字符计）
#[test]
fn rejects_too_long_description() {
    let text = format!("---\nname: tdd\ndescription: {}\n---\n", "字".repeat(1025));
    assert!(matches!(
        parse(&text, "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
}

/// name / description 仍必填
#[test]
fn rejects_missing_required_fields() {
    assert!(matches!(
        parse("---\nname: tdd\n---\n", "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
    assert!(matches!(
        parse("---\ndescription: 描述\n---\n", "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
    assert!(matches!(
        parse("正文\n", "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
}

/// metadata 里已有 version 行：整行重写，其余字段、正文与 CRLF 行尾逐字保留
#[test]
fn set_version_rewrites_the_metadata_version_line() {
    let text = "---\r\nname: tdd\r\ndescription: 测试驱动开发\r\nlicense: MIT\r\nmetadata:\r\n  source: owner/repo\r\n  version: \"1.0.0\"\r\n  tags: \"tdd\"\r\n---\r\n\r\n# tdd\r\n正文\r\n";
    let out = set_metadata_version(text, "1.1.0").unwrap();
    assert_eq!(
        out,
        text.replace("version: \"1.0.0\"", "version: \"1.1.0\""),
        "只换 metadata.version 行的值，其余字节不动"
    );
}

/// 块里有空行时，空行之后的 version 行照样是那一行：改写它，不另补第二条
#[test]
fn set_version_rewrites_version_line_across_blank_lines() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  source: owner/repo\n\n  version: \"1.0.0\"\n  tags: \"tdd\"\n---\n\n正文\n";
    let out = set_metadata_version(text, "1.1.0").unwrap();
    assert_eq!(
        out,
        text.replace("version: \"1.0.0\"", "version: \"1.1.0\""),
        "空行不该让已有 version 行漏检"
    );
    assert_eq!(out.matches("version:").count(), 1, "只能有一条 version 行");
}

/// metadata 块在但缺 version 行：插到该子块末尾，不越过下一个顶层字段
#[test]
fn set_version_inserts_at_metadata_block_end_when_missing() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  source: owner/repo\nlicense: MIT\n---\n\n正文\n";
    let out = set_metadata_version(text, "0.1.0").unwrap();
    assert_eq!(
        out,
        "---\nname: tdd\ndescription: 描述\nmetadata:\n  source: owner/repo\n  version: \"0.1.0\"\nlicense: MIT\n---\n\n正文\n"
    );
}

/// 子键缩进不是常见两空格时，补出来的行沿用块内既有缩进
#[test]
fn set_version_keeps_the_block_indent_when_it_is_unusual() {
    let text =
        "---\nname: tdd\ndescription: 描述\nmetadata:\n    source: owner/repo\n---\n\n正文\n";
    let out = set_metadata_version(text, "0.1.0").unwrap();
    assert_eq!(
        out,
        "---\nname: tdd\ndescription: 描述\nmetadata:\n    source: owner/repo\n    version: \"0.1.0\"\n---\n\n正文\n"
    );
}

/// 没有 metadata 块：在头部区末尾补出该块与一行，沿用文件既有行尾风格
#[test]
fn set_version_appends_metadata_block_when_absent() {
    let text = "---\r\nname: tdd\r\ndescription: 描述\r\n---\r\n\r\n正文\r\n";
    let out = set_metadata_version(text, "0.1.0").unwrap();
    assert_eq!(
        out,
        "---\r\nname: tdd\r\ndescription: 描述\r\nmetadata:\r\n  version: \"0.1.0\"\r\n---\r\n\r\n正文\r\n"
    );
}

/// 行内 metadata 不是映射，版本行无处可落：拒绝写回
#[test]
fn set_version_rejects_inline_metadata() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata: {version: \"1.0.0\"}\n---\n";
    assert!(matches!(
        set_metadata_version(text, "1.1.0"),
        Err(SkillError::InvalidSource { .. })
    ));
}

/// 没有 frontmatter 头部区时拒绝写回
#[test]
fn set_version_rejects_text_without_frontmatter() {
    assert!(matches!(
        set_metadata_version("正文\n", "1.0.0"),
        Err(SkillError::InvalidSource { .. })
    ));
    assert!(matches!(
        set_metadata_version("---\nname: tdd\n", "1.0.0"),
        Err(SkillError::InvalidSource { .. })
    ));
}

/// 标量值里的 `#` 注释被剥掉，值本身的 `#` 保留
#[test]
fn unquote_strips_trailing_comment_only() {
    assert_eq!(unquote("tdd # 注释"), "tdd");
    assert_eq!(unquote("\"a # b\""), "a # b");
    assert_eq!(unquote("c#sharp"), "c#sharp");
}
