//! frontmatter 测试：metadata 子块的字段解析、identity 两字段的规范
//! 约束、版本格式约束与写回。

use super::*;
use crate::skill::frontmatter::{parse, set_metadata_version};

/// frontmatter 读取：metadata 子块里的五个字段各就各位
#[test]
fn frontmatter_reads_five_fields_from_metadata_block() {
    let text = "---\nname: tdd\ndescription: 测试驱动开发\nmetadata:\n  version: \"1.2.0\"\n  source: mattpocock/skills\n  tags: [tdd, office]\n---\n\n正文\n";
    let fm = parse(text, "SKILL.md").unwrap();
    assert_eq!(fm.name, "tdd");
    assert_eq!(fm.description, "测试驱动开发");
    assert_eq!(fm.version.as_deref(), Some("1.2.0"));
    assert_eq!(fm.source.as_deref(), Some("mattpocock/skills"));
    assert_eq!(fm.tags, vec!["tdd", "office"]);
}

/// tags 的列表写法：流式与块式逐项成清单；字符串等非列表形态被库拒绝
#[test]
fn tags_must_be_a_list() {
    for tags in [
        "---\nname: tdd\ndescription: 描述\nmetadata:\n  tags: [tdd, office]\n---\n",
        "---\nname: tdd\ndescription: 描述\nmetadata:\n  tags:\n    - tdd\n    - office\n---\n",
    ] {
        assert_eq!(parse(tags, "SKILL.md").unwrap().tags, vec!["tdd", "office"]);
    }
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  tags: \"tdd, office\"\n---\n";
    assert!(
        matches!(
            parse(text, "SKILL.md"),
            Err(SkillError::InvalidSource { .. })
        ),
        "字符串 tags 应被拒绝"
    );
}

/// tags 里的空标签直接丢弃
#[test]
fn drops_empty_tag() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  tags: [\"\", a]\n---\n";
    assert_eq!(parse(text, "SKILL.md").unwrap().tags, vec!["a"]);
}

/// version 裸值三段数字本身是字符串，不带引号照样通过
#[test]
fn version_accepts_unquoted_three_part_scalar() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n  version: 1.2.0\n---\n";
    assert_eq!(
        parse(text, "SKILL.md").unwrap().version.as_deref(),
        Some("1.2.0")
    );
}

/// version 被 YAML 推断成数字时拒绝：字段类型是字符串，数字交给库拒
#[test]
fn rejects_numeric_version() {
    for version in ["1.2", "120"] {
        let text =
            format!("---\nname: tdd\ndescription: 描述\nmetadata:\n  version: {version}\n---\n");
        assert!(
            matches!(
                parse(&text, "SKILL.md"),
                Err(SkillError::InvalidSource { .. })
            ),
            "数字 version 应被拒绝: {version}"
        );
    }
}

/// metadata 整块可缺省：只有 name / description 必填
#[test]
fn missing_metadata_block_is_allowed() {
    let fm = parse("---\nname: tdd\ndescription: 描述\n---\n", "SKILL.md").unwrap();
    assert_eq!(fm.name, "tdd");
    assert_eq!(fm.version, None);
    assert!(fm.tags.is_empty());
}

/// 非法 YAML 直接报错并点名文件：报错里带解析器给出的位置信息
#[test]
fn rejects_invalid_yaml_with_parser_error() {
    let err = parse("---\nname: [unclosed\n---\n", "SKILL.md").unwrap_err();
    let SkillError::InvalidSource { reason, .. } = &err else {
        panic!("非法 YAML 应报无效源: {err:?}");
    };
    assert!(
        reason.contains("解析失败"),
        "文案要说明 frontmatter 解析失败: {reason}"
    );
}

/// version 的规则：字符串值必须是三段纯数字
#[test]
fn rejects_version_that_is_not_semver() {
    for version in ["\"1.2\"", "\"v1.2.0\"", "v1.2.0"] {
        let text =
            format!("---\nname: tdd\ndescription: 描述\nmetadata:\n  version: {version}\n---\n");
        assert!(
            matches!(
                parse(&text, "SKILL.md"),
                Err(SkillError::InvalidSource { .. })
            ),
            "非法 version 应被拒绝: {version}"
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

/// metadata 里已有 version 行：整行重写，其余字段与正文逐字保留；
/// 空行排版与块内缩进不影响定位
#[test]
fn set_version_rewrites_the_metadata_version_line() {
    let text = "---\r\nname: tdd\r\ndescription: 测试驱动开发\r\nlicense: MIT\r\nmetadata:\r\n  source: owner/repo\r\n  version: \"1.0.0\"\r\n  tags: [tdd]\r\n---\r\n\r\n# tdd\r\n正文\r\n";
    let out = set_metadata_version(text, "1.1.0").unwrap();
    assert_eq!(
        out,
        text.replace("version: \"1.0.0\"", "version: \"1.1.0\""),
        "只换 metadata.version 行的值，其余字节不动"
    );
    assert_eq!(out.matches("version:").count(), 1, "只能有一条 version 行");
}

/// metadata 块在但缺 version 行：插到该子块末尾，沿用块内既有缩进，
/// 不越过下一个顶层字段
#[test]
fn set_version_inserts_at_metadata_block_end_when_missing() {
    let text = "---\nname: tdd\ndescription: 描述\nmetadata:\n    source: owner/repo\nlicense: MIT\n---\n\n正文\n";
    let out = set_metadata_version(text, "0.1.0").unwrap();
    assert_eq!(
        out,
        "---\nname: tdd\ndescription: 描述\nmetadata:\n    source: owner/repo\n    version: \"0.1.0\"\nlicense: MIT\n---\n\n正文\n"
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
