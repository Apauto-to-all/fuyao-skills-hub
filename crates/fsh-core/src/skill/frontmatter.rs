//! SKILL.md frontmatter：手写 YAML 子集（顶层标量 + 缩进一层的 metadata 子块）
//! 的读取与 metadata.version 行写回。纯文本进、纯文本出，不碰 git、不碰磁盘。
//! 「顶层只认官方 name / description，fsh 自有的 version / source / tags 住在
//! metadata 下」这条形状规则、以及 identity 两字段的规范约束，唯一住所在本模块。

// shortcut: 手写 YAML 子集（顶层标量 + 一层缩进的 metadata 子块），不引入 YAML 依赖；
// 需要更深嵌套或完整 YAML 语法时换正经解析库

use super::{Frontmatter, SkillError, invalid};
use crate::version::Version;

/// fsh 自有字段：只认 metadata 下的位置，平铺在顶层即错位置
const OWN_FIELDS: [&str; 3] = ["version", "source", "tags"];

/// name 的长度上限（规范值）
const NAME_MAX_CHARS: usize = 64;

/// description 的长度上限（规范值）
const DESCRIPTION_MAX_CHARS: usize = 1024;

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
        let top_level = !is_indented(line);
        let blank = line.trim().is_empty();
        // 顶格 `- x` 是上一字段的块式列表项（YAML 允许列表项与键同列），
        // 空行是排版不是字段边界，两者都不算新字段；
        // 其余顶格行一出现即说明本字段已收束
        if top_level && !blank && !line.starts_with('-') {
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
pub(super) fn unquote(raw: &str) -> String {
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

/// metadata 写成行内值（不是映射）时的统一报错：解析与写回同一口径、同一文案
fn metadata_not_a_block(path: &str) -> SkillError {
    invalid(
        path,
        "metadata 必须是缩进块（子键写在 metadata: 下面）".into(),
    )
}

/// metadata 子块：剥掉公共缩进后，子键在读法与写回定位上都与顶层字段同形
/// （同一套 field_lines 直接可用，缩进一律取块内最小缩进）。
/// 行内值没有子键可读，直接点名拒绝而不是静默当空——写回版本号也只落进缩进块
fn metadata_block<'a>(block: &[&'a str], path: &str) -> Result<Vec<&'a str>, SkillError> {
    let Some(group) = field_lines(block, "metadata") else {
        return Ok(Vec::new());
    };
    if !inline_value(group[0]).is_empty() {
        return Err(metadata_not_a_block(path));
    }
    let sub: Vec<&str> = group[1..]
        .iter()
        .copied()
        .filter(|line| is_indented(line))
        .collect();
    let indent = sub
        .iter()
        .map(|line| line_indent(line).len())
        .min()
        .unwrap_or(0);
    Ok(sub.into_iter().map(|line| &line[indent..]).collect())
}

/// name 按规范校验：1–64 字符、只允许小写字母数字与连字符、首尾非连字符、不得连续连字符
fn check_name(name: &str, path: &str) -> Result<(), SkillError> {
    let chars = name.chars().count();
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

/// description 按规范校验：非空（缺失已在取值处拦）且不超过 1024 字符
fn check_description(description: &str, path: &str) -> Result<(), SkillError> {
    let chars = description.chars().count();
    if chars > DESCRIPTION_MAX_CHARS {
        return Err(invalid(
            path,
            format!("description 超过 {DESCRIPTION_MAX_CHARS} 字符（当前 {chars} 字符）"),
        ));
    }
    Ok(())
}

/// metadata.version：必须写成带引号的 x.y.z；裸值会被 YAML 当数字，号段被吃掉
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
            format!(
                "metadata.version 必须带引号（version: \"1.2.0\"）——裸值 {head} 会被 YAML 当数字解析"
            ),
        ));
    }
    let value = unquote(head);
    if Version::parse(&value).is_none() {
        return Err(invalid(
            path,
            format!("metadata.version 不是三段纯数字（x.y.z）: {value}"),
        ));
    }
    Ok(Some(value))
}

/// metadata.tags：只认一个逗号分隔的字符串（按逗号拆、去首尾空格、丢空项）；
/// 规范的 metadata 值是字符串，YAML 列表超出规范，流式与块式一并拒绝
fn tags_field(block: &[&str], path: &str) -> Result<Vec<String>, SkillError> {
    let Some(group) = field_lines(block, "tags") else {
        return Ok(Vec::new());
    };
    let head = unquote(group[0]);
    // 块式列表项的判据是 `- ` 开头的续行；空行只是排版，不算条目
    let block_list = group[1..]
        .iter()
        .any(|line| line.trim_start().starts_with('-'));
    if head.starts_with('[') || block_list {
        return Err(invalid(
            path,
            "metadata.tags 只能是逗号分隔的字符串（tags: \"tdd, office\"），不写 YAML 列表".into(),
        ));
    }
    Ok(head
        .split(',')
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect())
}

/// 读 frontmatter：顶层 name / description 必填并按规范校验，其余顶层字段不解析、
/// 随内容字节透传；fsh 自有的 version / source / tags 只从 metadata 子块读，
/// 平铺在顶层即报错并点名正确位置
pub(crate) fn parse(text: &str, path: &str) -> Result<Frontmatter, SkillError> {
    let block = frontmatter_block(text)
        .ok_or_else(|| invalid(path, "缺少 frontmatter（--- 包围的头部区）".into()))?;
    for key in OWN_FIELDS {
        if field_lines(&block, key).is_some() {
            return Err(invalid(
                path,
                format!("frontmatter 顶层不得出现 {key}：fsh 自有字段写在 metadata 下（{key}: …）"),
            ));
        }
    }
    let name = scalar_field(&block, "name")
        .ok_or_else(|| invalid(path, "frontmatter 缺少 name 字段".into()))?;
    check_name(&name, path)?;
    let description = scalar_field(&block, "description")
        .ok_or_else(|| invalid(path, "frontmatter 缺少 description 字段".into()))?;
    check_description(&description, path)?;
    let meta = metadata_block(&block, path)?;
    Ok(Frontmatter {
        name,
        description,
        version: version_field(&meta, path)?,
        source: scalar_field(&meta, "source"),
        tags: tags_field(&meta, path)?,
    })
}

/// 把目标版本号写回 frontmatter 的 metadata.version：metadata 块里已有 version 行
/// 就整行重写；有 metadata 块但缺 version 行就插到该子块末尾；没有 metadata 块
/// 就在头部区末尾补出该块与一行。其余字段与正文一个字节不改——透传字段不丢
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
    // 行内值不是映射，版本行无处可落：与解析侧同一口径拒绝
    if !inline_value(lines[start].split_once(':').map(|(_, v)| v).unwrap_or("")).is_empty() {
        return Err(metadata_not_a_block("SKILL.md"));
    }
    // 子块范围：metadata 行之后到下一个顶格字段（或头部区收束）之间的行；
    // 空行算块内排版，否则插入点会落在空行之前、已有的 version 行搜不到，留下两条同名字段
    let stop = (start + 1..end)
        .take_while(|&i| is_indented(&lines[i]) || lines[i].trim().is_empty())
        .count()
        + start
        + 1;
    // 直接子键的缩进取块内最小缩进（解析侧也按最小缩进去缩进，两侧口径必须一致）；
    // 块内还没有子键时按两空格补，与手写 frontmatter 的常见写法一致
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
