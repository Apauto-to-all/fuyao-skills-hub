//! agent 技能目录的写法解析：把配置里的可移植写法展开成绝对路径。
//! 只认三类——开头的裸 `~`、环境变量（`%NAME%` 与 `$NAME` / `${NAME}`）、
//! 绝对路径；结果必须是绝对路径且不含 `..`，未设置的变量报错，绝不把变量名
//! 留成字面量。家目录与变量取值由调用方注入，本模块测试不碰真实环境。

use std::path::{Component, Path, PathBuf};

/// 展开一个可移植目录写法；失败原因是给人读的一句话（调用方补字段名）
pub(super) fn expand(
    raw: &str,
    home: &Path,
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("目录不能为空".into());
    }
    let expanded = expand_env(raw, lookup)?;
    let path = match expanded.strip_prefix('~') {
        None => PathBuf::from(expanded),
        // 只认开头的裸 `~`：`~user` 不是本项目写法，宁可报错也不猜
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            match rest.trim_start_matches(['/', '\\']) {
                "" => home.to_path_buf(),
                rest => home.join(rest),
            }
        }
        Some(_) => return Err(format!("只认开头的 `~`，不认 `~user` 形式: {raw}")),
    };
    if !path.is_absolute() {
        return Err(format!(
            "必须是绝对路径、`~` 前缀或环境变量写法: {raw}（展开为 {}）",
            path.display()
        ));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("不得包含 `..`: {raw}"));
    }
    Ok(path.to_path_buf())
}

/// 展开两种环境变量写法；未设置或值为空的变量一律报错
fn expand_env(raw: &str, lookup: &impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '%' => match chars[i + 1..].iter().position(|c| *c == '%') {
                // 成对出现才是变量；落单的 `%` 按普通字符留在路径里
                Some(offset) => {
                    let end = i + 1 + offset;
                    let name: String = chars[i + 1..end].iter().collect();
                    out.push_str(&value_of(&name, lookup, raw)?);
                    i = end + 1;
                }
                None => {
                    out.push('%');
                    i += 1;
                }
            },
            '$' => match parse_dollar(&chars, i, raw)? {
                Some((name, next)) => {
                    out.push_str(&value_of(&name, lookup, raw)?);
                    i = next;
                }
                // 不成变量的 `$` 原样留下：Windows 路径里可能有字面 `$`（如 `c$`）
                None => {
                    out.push('$');
                    i += 1;
                }
            },
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// 解析 `$NAME` 或 `${NAME}`，返回变量名与下一个待扫描下标；
/// 不成变量时返回 None，由调用方把 `$` 原样留下
fn parse_dollar(
    chars: &[char],
    start: usize,
    raw: &str,
) -> Result<Option<(String, usize)>, String> {
    if let Some('{') = chars.get(start + 1) {
        let Some(offset) = chars[start + 2..].iter().position(|c| *c == '}') else {
            return Err(format!("环境变量写法不完整（缺 `}}`）: {raw}"));
        };
        let end = start + 2 + offset;
        let name: String = chars[start + 2..end].iter().collect();
        if name.is_empty() {
            return Err(format!("环境变量名不能为空: {raw}"));
        }
        return Ok(Some((name, end + 1)));
    }
    let end = start
        + 1
        + chars[start + 1..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
            .count();
    if end == start + 1 {
        return Ok(None);
    }
    Ok(Some((chars[start + 1..end].iter().collect(), end)))
}

/// 取一个环境变量的值：未设置或空值都算未设置
fn value_of(
    name: &str,
    lookup: &impl Fn(&str) -> Option<String>,
    raw: &str,
) -> Result<String, String> {
    if name.is_empty() {
        return Err(format!("环境变量名不能为空: {raw}"));
    }
    lookup(name)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("环境变量 {name} 未设置: {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 展开夹具：家目录与变量取值都由测试注入，不碰真实环境
    fn expand_raw(raw: &str) -> Result<PathBuf, String> {
        let lookup = |name: &str| match name {
            "FSH_TEST_DIR" => Some("D:/shared".to_string()),
            _ => None,
        };
        expand(raw, Path::new("C:/home/u"), &lookup)
    }

    #[test]
    fn expands_tilde_prefix_against_home() {
        assert_eq!(
            expand_raw("~/.claude/skills").unwrap(),
            Path::new("C:/home/u/.claude/skills")
        );
        assert_eq!(expand_raw("~").unwrap(), Path::new("C:/home/u"));
    }

    #[test]
    fn expands_both_env_var_forms() {
        for raw in [
            "%FSH_TEST_DIR%/skills",
            "$FSH_TEST_DIR/skills",
            "${FSH_TEST_DIR}/skills",
        ] {
            assert_eq!(
                expand_raw(raw).unwrap(),
                Path::new("D:/shared/skills"),
                "{raw} 展开结果不对"
            );
        }
    }

    #[test]
    fn absolute_path_passes_through_unchanged() {
        assert_eq!(
            expand_raw("D:/plain/skills").unwrap(),
            Path::new("D:/plain/skills")
        );
    }

    #[test]
    fn keeps_literal_dollar_and_rejects_empty_variable_names() {
        // 字面 `$` 必须原样留下：UNC 管理共享 `c$` 里就有它
        let unc = r"\\srv\c$\skills";
        assert_eq!(expand_raw(unc).unwrap(), Path::new(unc));
        for raw in ["${}/skills", "%%/skills", "${NAME/skills"] {
            assert!(expand_raw(raw).is_err(), "{raw} 应被拒绝");
        }
    }

    #[test]
    fn rejects_unset_variables_relative_and_parent_segments() {
        for raw in [
            "%FSH_TEST_UNSET_9F3%/skills",
            "$FSH_TEST_UNSET_9F3/skills",
            "relative/skills",
            "~user/skills",
            "~/../escape",
            "   ",
        ] {
            assert!(expand_raw(raw).is_err(), "{raw} 应被拒绝");
        }
        let err = expand_raw("%FSH_TEST_UNSET_9F3%/skills").unwrap_err();
        assert!(err.contains("FSH_TEST_UNSET_9F3"), "报错要点名变量: {err}");
    }
}
