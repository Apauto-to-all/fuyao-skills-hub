//! frontmatter 解析测试：只解析 name / description。

use crate::skill::frontmatter::parse;

#[test]
fn frontmatter_reads_name_and_description() {
    let text = "---\nname: tdd\ndescription: 测试驱动开发\n---\n\n正文\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.name, "tdd");
    assert_eq!(fm.description, "测试驱动开发");
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
