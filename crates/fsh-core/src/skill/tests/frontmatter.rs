//! frontmatter 解析测试：只解析 name / description / source，
//! 其余字段（含 metadata.version）不解析、随内容透传。

use crate::skill::frontmatter::parse;

#[test]
fn frontmatter_reads_name_description_source() {
    let text = "---\nname: tdd\ndescription: 测试驱动开发\nmetadata:\n  source: mattpocock/skills\n---\n\n正文\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.name, "tdd");
    assert_eq!(fm.description, "测试驱动开发");
    assert_eq!(fm.source.as_deref(), Some("mattpocock/skills"));
}

/// metadata.version 完全不解析：任何形态（合法号、垃圾值、数字）都不影响解析
#[test]
fn version_field_is_ignored_entirely() {
    for version in ["\"1.2.0\"", "garbage", "1.2"] {
        let text =
            format!("---\nname: tdd\ndescription: 描述\nmetadata:\n  version: {version}\n---\n");
        let fm = parse(&text, "SKILL.md").unwrap();
        assert_eq!(fm.name, "tdd", "version 不解析就不校验: {version}");
    }
}

/// 缺 metadata 块是合法形态：source 为 None
#[test]
fn missing_metadata_block_is_allowed() {
    let text = "---\nname: tdd\ndescription: 描述\n---\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.source, None);
}

#[test]
fn rejects_invalid_yaml_with_parser_error() {
    let text = "---\nname: [broken\ndescription: 描述\n---\n";
    assert!(parse(text, "SKILL.md").is_err());
}

/// name / description 缺失或非法时点名字段
#[test]
fn rejects_missing_or_invalid_required_fields() {
    for (text, field) in [
        ("---\ndescription: 描述\n---\n", "name"),
        ("---\nname: tdd\n---\n", "description"),
    ] {
        let err = parse(text, "SKILL.md").unwrap_err().to_string();
        assert!(err.contains(field), "报错要点名字段: {err}");
    }
    assert!(parse("无 frontmatter\n", "SKILL.md").is_err());
}
