//! frontmatter 测试：五字段解析、流式与块式列表等价、非法字段与版本拒绝。

use super::*;
use crate::skill::frontmatter::parse;

#[test]
fn frontmatter_reads_five_fields_and_equivalent_tag_syntax() {
    let flow = "---\nname: pdf\ndescription: 处理 PDF\nversion: \"1.2.0\"\nsource: anthropics/skills\ntags: [pdf, office]\nlicense: MIT\n---\n\n正文\n";
    let block = "---\nname: pdf\ndescription: 处理 PDF\nversion: \"1.2.0\"\nsource: anthropics/skills\ntags:\n  - pdf\n  - office\nlicense: MIT\n---\n\n正文\n";
    let a = parse(flow, "SKILL.md").unwrap();
    let b = parse(block, "SKILL.md").unwrap();
    assert_eq!(a, b, "tags 流式与块式应等价，透传字段不参与解析");
    // 块式列表项顶格书写也合法，同样算 tags 的内容
    let flush = "---\nname: pdf\ndescription: 处理 PDF\nversion: \"1.2.0\"\nsource: anthropics/skills\ntags:\n- pdf\n- office\nlicense: MIT\n---\n\n正文\n";
    assert_eq!(parse(flush, "SKILL.md").unwrap(), a);
    assert_eq!(a.name, "pdf");
    assert_eq!(a.description, "处理 PDF");
    assert_eq!(a.version.as_deref(), Some("1.2.0"));
    assert_eq!(a.source.as_deref(), Some("anthropics/skills"));
    assert_eq!(a.tags, vec!["pdf", "office"]);
}

#[test]
fn frontmatter_rejects_missing_fields_and_unquoted_or_bad_version() {
    for text in [
        "---\nname: pdf\ndescription: 描述\nversion: 1.2\n---\n",
        "---\nname: pdf\ndescription: 描述\nversion: 1.2.0\n---\n",
        "---\nname: pdf\ndescription: 描述\nversion: \"1.2\"\n---\n",
        "---\nname: pdf\ndescription: 描述\nversion: \"v1.2.0\"\n---\n",
    ] {
        assert!(
            matches!(
                parse(text, "SKILL.md"),
                Err(SkillError::InvalidSource { .. })
            ),
            "非法 version 应被拒绝: {text}"
        );
    }
    assert!(matches!(
        parse("---\nname: pdf\n---\n", "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
    assert!(matches!(
        parse("---\ndescription: 描述\n---\n", "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
}
