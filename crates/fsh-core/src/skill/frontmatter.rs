//! SKILL.md frontmatter 的读取。读侧由 YAML 解析器直达具名字段：
//! name / description 必填并按规范校验，source 取自 metadata 子块；
//! 其余字段（含 metadata.version）不解析、随内容透传。

use serde::Deserialize;

use super::{Frontmatter, SkillError, invalid};

/// name 的长度上限（规范值）
const NAME_MAX_CHARS: usize = 64;

/// description 的长度上限（规范值）
const DESCRIPTION_MAX_CHARS: usize = 1024;

/// YAML 中间结构：只声明本项目解析的字段，类型由 serde 按声明校验，
/// 其余键直接跳过
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    name: Option<String>,
    description: Option<String>,
    metadata: Option<Metadata>,
}

/// metadata 子块：同样只声明本项目解析的字段
#[derive(Debug, Default, Deserialize)]
struct Metadata {
    source: Option<String>,
}

/// name 按规范校验：非空、1–64 字符、只允许小写字母数字与连字符、首尾非连字符、不得连续连字符
fn check_name(name: &str, path: &str) -> Result<(), SkillError> {
    let chars = name.chars().count();
    if chars == 0 {
        return Err(invalid(path, "name 不得为空".into()));
    }
    if chars > NAME_MAX_CHARS {
        return Err(invalid(
            path,
            format!("name 超过 {NAME_MAX_CHARS} 字符（当前 {chars} 字符）"),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(invalid(
            path,
            format!("name 含非法字符（只允许小写字母、数字与连字符）: {name}"),
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(invalid(
            path,
            format!("name 不得以连字符开头或结尾，也不得出现连续连字符: {name}"),
        ));
    }
    Ok(())
}

/// description 按规范校验：非空且不超过 1024 字符
fn check_description(description: &str, path: &str) -> Result<(), SkillError> {
    let chars = description.chars().count();
    if chars == 0 {
        return Err(invalid(path, "description 不得为空".into()));
    }
    if chars > DESCRIPTION_MAX_CHARS {
        return Err(invalid(
            path,
            format!("description 超过 {DESCRIPTION_MAX_CHARS} 字符（当前 {chars} 字符）"),
        ));
    }
    Ok(())
}

/// frontmatter 头部区：首行 --- 与下一个 --- 之间的行（不含定界行）
fn frontmatter_block(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut block = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Some(block);
        }
        block.push(line);
    }
    None
}

/// 读 frontmatter：name / description 必填并按规范校验；source 取自
/// metadata 子块，缺省即无。YAML 语法或字段类型不对、字段校验失败都
/// 报错并点名文件
pub(crate) fn parse(text: &str, path: &str) -> Result<Frontmatter, SkillError> {
    let block = frontmatter_block(text)
        .ok_or_else(|| invalid(path, "缺少 frontmatter（--- 包围的头部区）".into()))?;
    let raw: RawFrontmatter = serde_saphyr::from_str(&block.join("\n"))
        .map_err(|e| invalid(path, format!("frontmatter 解析失败: {e}")))?;
    let name = raw
        .name
        .ok_or_else(|| invalid(path, "frontmatter 缺少 name 字段".into()))?;
    check_name(&name, path)?;
    let description = raw
        .description
        .ok_or_else(|| invalid(path, "frontmatter 缺少 description 字段".into()))?;
    check_description(&description, path)?;
    let meta = raw.metadata.unwrap_or_default();
    Ok(Frontmatter {
        name,
        description,
        source: meta.source,
    })
}
