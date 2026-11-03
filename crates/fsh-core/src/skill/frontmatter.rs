//! SKILL.md frontmatter：手写 YAML 子集（标量 + 流式/块式列表）的读取与
//! version 行写回。纯文本进、纯文本出，不碰 git、不碰磁盘。
//! 五字段规则与「其余字段字节透传」规则的唯一住所在本模块。

// shortcut: 手写 YAML 子集（标量 + 流式/块式列表），不引入 YAML 依赖；
// 需要嵌套结构或完整 YAML 语法时换正经解析库

use super::{Frontmatter, SkillError, invalid};

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

/// 取某顶层字段的原始行组：首行是冒号后的文本，后继行是它的续行
/// （缩进的块式列表，或与键同列的顶格 `- x` 列表项）；
/// 遇到下一个顶层字段即收束
fn field_lines<'a>(block: &[&'a str], key: &str) -> Option<Vec<&'a str>> {
    let mut group: Option<Vec<&'a str>> = None;
    for line in block {
        let top_level = !line.starts_with(' ') && !line.starts_with('\t');
        // 顶格 `- x` 是上一字段的块式列表项（YAML 允许列表项与键同列），
        // 不是新字段；其余顶格行一出现即说明本字段已收束
        if top_level && !line.starts_with('-') {
            if group.is_some() {
                return group;
            }
            let (k, v) = line.split_once(':').unwrap_or((line, ""));
            if k.trim() == key {
                group = Some(vec![v]);
            }
            continue;
        }
        if let Some(g) = group.as_mut() {
            g.push(line);
        }
    }
    group
}

/// 去引号与行尾注释：带引号取引号内文本，裸值截到 ` #`（YAML 注释起点）
fn unquote(raw: &str) -> String {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix('"') {
        return rest.split('"').next().unwrap_or_default().to_string();
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return rest.split('\'').next().unwrap_or_default().to_string();
    }
    match s.find(" #") {
        Some(i) => s[..i].trim_end().to_string(),
        None => s.to_string(),
    }
}

/// 标量字段值（name / description / source）：取首行并剥引号与注释
fn scalar_field(block: &[&str], key: &str) -> Option<String> {
    let group = field_lines(block, key)?;
    let value = unquote(group.first()?);
    (!value.is_empty()).then_some(value)
}

/// 列表字段值（tags）：流式 `[a, b]` 与块式 `- a` 等价，元素剥引号
fn list_field(block: &[&str], key: &str) -> Vec<String> {
    let Some(group) = field_lines(block, key) else {
        return Vec::new();
    };
    let head = unquote(group[0]);
    if let Some(inner) = head.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return inner
            .split(',')
            .map(unquote)
            .filter(|item| !item.is_empty())
            .collect();
    }
    group[1..]
        .iter()
        .filter_map(|line| line.trim().strip_prefix('-'))
        .map(unquote)
        .filter(|item| !item.is_empty())
        .collect()
}

/// 读 frontmatter 顶层五字段：缺 name/description、version 未带引号或非
/// 三段纯数字均报 InvalidSource；其余顶层字段不解析（写回也只碰 version 行）
pub(super) fn parse(text: &str, path: &str) -> Result<Frontmatter, SkillError> {
    let block = frontmatter_block(text)
        .ok_or_else(|| invalid(path, "缺少 frontmatter（--- 包围的头部区）".into()))?;
    let name = scalar_field(&block, "name")
        .ok_or_else(|| invalid(path, "frontmatter 缺少 name 字段".into()))?;
    let description = scalar_field(&block, "description")
        .ok_or_else(|| invalid(path, "frontmatter 缺少 description 字段".into()))?;
    Ok(Frontmatter {
        name,
        description,
        version: version_field(&block, path)?,
        source: scalar_field(&block, "source"),
        tags: list_field(&block, "tags"),
    })
}

/// version 字段：必须写成带引号的 x.y.z；裸值会被 YAML 当数字，号段被吃掉
fn version_field(block: &[&str], path: &str) -> Result<Option<String>, SkillError> {
    let Some(group) = field_lines(block, "version") else {
        return Ok(None);
    };
    let head = group[0].trim();
    if head.is_empty() {
        return Ok(None);
    }
    if !(head.starts_with('"') || head.starts_with('\'')) {
        return Err(invalid(
            path,
            format!("version 必须带引号（version: \"1.2.0\"）——裸值 {head} 会被 YAML 当数字解析"),
        ));
    }
    let value = unquote(head);
    if !valid_semver(&value) {
        return Err(invalid(
            path,
            format!("version 不是三段纯数字（x.y.z）: {value}"),
        ));
    }
    Ok(Some(value))
}

/// 校验 x.y.z 三段纯数字版本号（naming 的 tag 反解也用）
pub(crate) fn valid_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u64>().is_ok())
}

/// 三段纯数字版本号比较（号只能前进；两侧都已过 valid_semver 校验）
pub(super) fn cmp_semver(a: &str, b: &str) -> std::cmp::Ordering {
    let seg = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    seg(a).cmp(&seg(b))
}

/// 把目标版本号写回 frontmatter：只换 version 行的值，缺行则在头部区末尾补一行；
/// 其余字段与正文一个字节不改——透传字段不丢
pub(super) fn set_frontmatter_version(text: &str, version: &str) -> Result<String, SkillError> {
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
    for i in 1..end {
        let line = &lines[i];
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let (k, _) = line.split_once(':').unwrap_or((line.as_str(), ""));
        // version 行由 fsh 独占：整行重写为 fsh 生成的形式，对齐与行尾注释不保留
        if k.trim() == "version" {
            let cr = if line.ends_with('\r') { "\r" } else { "" };
            lines[i] = format!("version: \"{version}\"{cr}");
            return Ok(lines.join("\n"));
        }
    }
    // 补在头部区末尾：不插进任何字段的行组里，块标量（`|` / `>`）不会被拆散；
    // 行尾沿用文件既有风格（CRLF 文件补出来的行也带 \r）
    let eol = if lines.iter().any(|l| l.ends_with('\r')) {
        "\r"
    } else {
        ""
    };
    lines.insert(end, format!("version: \"{version}\"{eol}"));
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写回版本号时，其余顶层字段、正文与 CRLF 行尾逐字保留
    #[test]
    fn set_version_rewrites_only_the_version_line() {
        let text = "---\r\nname: pdf\r\ndescription: 处理 PDF\r\nversion: \"1.0.0\"\r\nlicense: MIT\r\ntags: [pdf]\r\n---\r\n\r\n# pdf\r\n正文\r\n";
        let out = set_frontmatter_version(text, "1.1.0").unwrap();
        assert_eq!(
            out,
            text.replace("version: \"1.0.0\"", "version: \"1.1.0\""),
            "只换 version 行的值，其余字节不动"
        );
    }

    /// 缺 version 行时补在头部区末尾，沿用文件既有行尾风格
    #[test]
    fn set_version_inserts_line_at_block_end_when_missing() {
        let text = "---\r\nname: pdf\r\ndescription: 描述\r\n---\r\n\r\n正文\r\n";
        let out = set_frontmatter_version(text, "0.1.0").unwrap();
        assert_eq!(
            out,
            "---\r\nname: pdf\r\ndescription: 描述\r\nversion: \"0.1.0\"\r\n---\r\n\r\n正文\r\n"
        );
    }

    /// 没有 frontmatter 头部区时拒绝写回
    #[test]
    fn set_version_rejects_text_without_frontmatter() {
        assert!(matches!(
            set_frontmatter_version("正文\n", "1.0.0"),
            Err(SkillError::InvalidSource { .. })
        ));
        assert!(matches!(
            set_frontmatter_version("---\nname: pdf\n", "1.0.0"),
            Err(SkillError::InvalidSource { .. })
        ));
    }

    /// 标量值里的 `#` 注释被剥掉，值本身的 `#` 保留
    #[test]
    fn unquote_strips_trailing_comment_only() {
        assert_eq!(unquote("pdf # 注释"), "pdf");
        assert_eq!(unquote("\"a # b\""), "a # b");
        assert_eq!(unquote("c#sharp"), "c#sharp");
    }
}
