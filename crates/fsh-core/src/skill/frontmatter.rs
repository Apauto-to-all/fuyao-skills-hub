//! SKILL.md frontmatter 的读取与版本行写回。读侧由 YAML 解析器直达
//! 具名字段：name / description 必填并按规范校验，version / source /
//! tags 取自 metadata 子块，其余键一律跳过。写回是行级文本手术：只
//! 替换 metadata 子块里的 version 行，其余字段与正文逐字保留。

use serde::Deserialize;

use super::{Frontmatter, SkillError, invalid};
use crate::version::Version;

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
    version: Option<String>,
    source: Option<String>,
    tags: Option<Vec<String>>,
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

/// version：读出后必须是三段数字（x.y.z）；数字形态在反序列化时就被拒
fn version_field(text: &str, path: &str) -> Result<String, SkillError> {
    if Version::parse(text).is_none() {
        return Err(invalid(
            path,
            format!("metadata.version 不是三段纯数字（x.y.z）: {text}"),
        ));
    }
    Ok(text.to_string())
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

/// 读 frontmatter：name / description 必填并按规范校验；version / source /
/// tags 取自 metadata 子块，缺省即无。YAML 语法或字段类型不对、字段校验
/// 失败都报错并点名文件
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
    let mut tags = meta.tags.unwrap_or_default();
    tags.retain(|tag| !tag.is_empty());
    Ok(Frontmatter {
        name,
        description,
        version: meta.version.map(|v| version_field(&v, path)).transpose()?,
        source: meta.source,
        tags,
    })
}

/// 一行是否带缩进（空格或制表符开头）
fn is_indented(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

/// 一行的前导缩进（空格与制表符）
fn line_indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// 字段行首行的行内值：剥掉行尾注释后的原样文本（引号本身算字面量）。
/// 空串说明这个字段没有行内值，是块式写法的开头
fn inline_value(raw: &str) -> &str {
    let s = raw.trim();
    if s.starts_with('#') {
        return "";
    }
    match s.find(" #") {
        Some(i) => s[..i].trim_end(),
        None => s,
    }
}

/// 顶层字段行判定：顶格写、且冒号前的键名相等
fn is_top_field(line: &str, key: &str) -> bool {
    !is_indented(line) && line.split(':').next().map(str::trim) == Some(key)
}

/// 把目标版本号写回 frontmatter 的 metadata.version：metadata 块里已有
/// version 行就整行重写；有 metadata 块但缺 version 行就插到该子块末尾；
/// 没有 metadata 块就在头部区末尾补出该块与一行。其余字段与正文
/// 一个字节不改——透传字段不丢
pub(super) fn set_metadata_version(text: &str, version: &str) -> Result<String, SkillError> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return Err(invalid(
            "SKILL.md",
            "缺少 frontmatter（--- 包围的头部区）".into(),
        ));
    }
    // 头部区收束在下一个 `---`；没有收束行即无 frontmatter
    let end = lines
        .iter()
        .skip(1)
        .position(|l| l.trim() == "---")
        .map(|i| i + 1)
        .ok_or_else(|| invalid("SKILL.md", "缺少 frontmatter（--- 包围的头部区）".into()))?;
    // 行尾沿用文件既有风格（CRLF 文件补出来的行也带 \r）
    let eol = if lines.iter().any(|l| l.ends_with('\r')) {
        "\r"
    } else {
        ""
    };
    let Some(start) = (1..end).find(|&i| is_top_field(&lines[i], "metadata")) else {
        lines.insert(end, format!("metadata:{eol}"));
        lines.insert(end + 1, format!("  version: \"{version}\"{eol}"));
        return Ok(lines.join("\n"));
    };
    // metadata 行内值不是映射，子键无处可落：整体拒绝写回
    if !inline_value(lines[start].split_once(':').map(|(_, v)| v).unwrap_or("")).is_empty() {
        return Err(invalid(
            "SKILL.md",
            "metadata 必须是缩进块（子键写在 metadata: 下面）".into(),
        ));
    }
    // 子块范围：metadata 行之后到下一个顶格字段（或头部区收束）之间的行；
    // 空行算块内排版，否则插入点会落在空行之前、已有的 version 行搜不到，留下两条同名字段
    let stop = (start + 1..end)
        .take_while(|&i| is_indented(&lines[i]) || lines[i].trim().is_empty())
        .count()
        + start
        + 1;
    // 直接子键的缩进取块内最小缩进；块内还没有子键时按两空格补，
    // 与手写 frontmatter 的常见写法一致
    let indent = (start + 1..stop)
        .map(|i| line_indent(&lines[i]))
        .filter(|ws| !ws.is_empty())
        .min_by_key(|ws| ws.len())
        .unwrap_or("  ")
        .to_string();
    // 直接子键里的 version 行：整行重写为 fsh 生成的形式，该行的对齐与行尾注释不保留；
    // 缩进沿用块内既有写法，原有 CR 保留
    let existing = (start + 1..stop).find(|&i| {
        line_indent(&lines[i]) == indent
            && lines[i].trim_start().split(':').next().map(str::trim) == Some("version")
    });
    match existing {
        Some(i) => {
            let cr = if lines[i].ends_with('\r') { "\r" } else { "" };
            lines[i] = format!("{indent}version: \"{version}\"{cr}");
        }
        None => lines.insert(stop, format!("{indent}version: \"{version}\"{eol}")),
    }
    Ok(lines.join("\n"))
}
