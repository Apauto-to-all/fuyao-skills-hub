//! 选择器：技能键不再由用户手写，改由「前缀 + 技能名」两段拼出。
//! 前缀是组织归属（可多段，省略即根），技能名恒为一段；一次选择点名的是
//! 一个前缀下的一套技能的子集，`--all` 表示这套里的全部（含更深的层）。
//! 解析是纯函数：前缀 + 该前缀下可用的键 + 点名的名字 → 要动的键清单；
//! 全部吃技能键的动词共用这一份规则，坏选择一律在这里拒绝，不落到盘面判定。

use thiserror::Error;

use crate::ErrorCode;
use crate::error::CommonError;
use crate::key::KeyError;

/// 整套的展示名：省略前缀即根
pub const ROOT_LABEL: &str = "根";

/// 一次技能选择：前缀配点名的名字，或前缀配「全部」
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    /// 组织前缀（已归一：空串与全空白按未给处理）；None 即根
    prefix: Option<String>,
    /// 点名的技能名；None 表示整套
    names: Option<Vec<String>>,
}

impl Selector {
    /// 按名字挑：名字恒为一段，重复的静默去重
    pub fn named(prefix: Option<String>, names: Vec<String>) -> Self {
        Self {
            prefix: normalize(prefix.as_deref()).map(str::to_string),
            names: Some(names),
        }
    }

    /// 整套：该前缀下的全部技能
    pub fn all(prefix: Option<String>) -> Self {
        Self {
            prefix: normalize(prefix.as_deref()).map(str::to_string),
            names: None,
        }
    }

    /// 归一后的前缀；None 即根
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// 整套的展示名：前缀原样，省略即「根」。提交消息与日志用它
    pub fn label(&self) -> &str {
        self.prefix.as_deref().unwrap_or(ROOT_LABEL)
    }

    /// 形状检查：前缀是多段键的形状，技能名恒为一段。不碰盘，先于任何盘面判定
    pub fn check(&self) -> Result<(), CommonError> {
        if let Some(prefix) = &self.prefix {
            crate::key::validate(prefix)?;
        }
        for name in self.names.iter().flatten() {
            name_shape(name)?;
        }
        Ok(())
    }

    /// 解析：点名的每个名字拼成键后必须在该前缀下可用；`--all` 取全部可用键。
    /// `available` 是调用方在该前缀下扫到的技能键，解析因此纯而可测
    pub fn resolve(&self, available: &[String]) -> Result<Vec<String>, CommonError> {
        self.check()?;
        let Some(names) = &self.names else {
            return Ok(available.to_vec());
        };
        let mut keys: Vec<String> = Vec::new();
        for name in names {
            let key = match &self.prefix {
                Some(prefix) => format!("{prefix}/{name}"),
                None => name.clone(),
            };
            // 同一个键只动一次：重复点名静默去重
            if keys.contains(&key) {
                continue;
            }
            if !available.contains(&key) {
                return Err(NoMatch {
                    key,
                    available: available_names(available, self.prefix()),
                }
                .into());
            }
            keys.push(key);
        }
        Ok(keys)
    }
}

/// 前缀归一：空串与全空白视同未给（=根）
pub fn normalize(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|prefix| !prefix.is_empty())
}

/// 键是否在该前缀的套里：前缀按段边界认（`k1/k2` 不命中 `k1/k2b`），
/// 且前缀之下至少要有一段；省略前缀即根，全部键都在根下
pub fn in_set(key: &str, prefix: Option<&str>) -> bool {
    match prefix {
        Some(prefix) => key
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.len() > 1 && rest.starts_with('/')),
        None => true,
    }
}

/// 一个面里某个前缀下的全部技能键，按键排序
pub fn members(keys: impl IntoIterator<Item = String>, prefix: Option<&str>) -> Vec<String> {
    let mut keys: Vec<String> = keys.into_iter().filter(|key| in_set(key, prefix)).collect();
    keys.sort();
    keys
}

/// 点名的技能在该前缀下不存在：该前缀下可用的技能随行，一次就能改对
#[derive(Debug, Error)]
#[error("该前缀下没有技能: {key}（可用的技能: {available}）")]
pub struct NoMatch {
    /// 拼出来的整键
    pub key: String,
    /// 该前缀下可用的技能（相对前缀的路径，顿号分隔；空集写「无」）
    pub available: String,
}

impl ErrorCode for NoMatch {
    fn error_code(&self) -> &'static str {
        "no_match"
    }
}

/// 技能名形状：恒为一段，只收小写字母、数字与连字符。
/// 名字里写斜杠是把「放在哪一套」混进了身份，报错直接教它写进前缀
fn name_shape(name: &str) -> Result<(), KeyError> {
    let reason = if name.contains('/') || name.contains('\\') {
        "技能名恒为一段，分层请写进 --prefix"
    } else if name.is_empty() {
        "技能名不能为空"
    } else if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        "技能名只能是小写字母、数字与连字符"
    } else {
        return Ok(());
    };
    Err(KeyError {
        key: name.to_string(),
        reason,
    })
}

/// 可用技能的展示名：相对前缀的路径，空集写「无」
fn available_names(available: &[String], prefix: Option<&str>) -> String {
    if available.is_empty() {
        return "无".to_string();
    }
    available
        .iter()
        .map(|key| match prefix {
            Some(prefix) => key
                .strip_prefix(prefix)
                .unwrap_or(key)
                .trim_start_matches('/'),
            None => key.as_str(),
        })
        .collect::<Vec<_>>()
        .join("、")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(params: &[&str]) -> Vec<String> {
        params.iter().map(|key| key.to_string()).collect()
    }

    /// 多段前缀 + 单段技能名：键由两段拼出
    #[test]
    fn resolve_joins_prefix_and_name() {
        let available = keys(&["k1/k2/tdd", "k1/k2/db/sql"]);
        let sel = Selector::named(Some("k1/k2".into()), keys(&["tdd"]));
        assert_eq!(sel.resolve(&available).unwrap(), keys(&["k1/k2/tdd"]));
    }

    /// 省略前缀即根：键就是技能名自己
    #[test]
    fn resolve_at_root_uses_the_name_as_key() {
        let available = keys(&["demo", "k1/k2/tdd"]);
        let sel = Selector::named(None, keys(&["demo"]));
        assert_eq!(sel.resolve(&available).unwrap(), keys(&["demo"]));
    }

    /// 空串与全空白前缀按「未给」处理
    #[test]
    fn blank_prefix_is_the_root() {
        for blank in ["", "   "] {
            let sel = Selector::named(Some(blank.into()), keys(&["demo"]));
            assert_eq!(sel.prefix(), None, "{blank:?} 应视同未给");
            assert_eq!(sel.label(), ROOT_LABEL);
            assert_eq!(sel.resolve(&keys(&["demo"])).unwrap(), keys(&["demo"]));
        }
    }

    /// 重复点名的技能名静默去重：同一个键只出现一次
    #[test]
    fn repeated_names_are_deduped() {
        let sel = Selector::named(Some("k1".into()), keys(&["a", "a", "b"]));
        assert_eq!(
            sel.resolve(&keys(&["k1/a", "k1/b"])).unwrap(),
            keys(&["k1/a", "k1/b"])
        );
    }

    /// 名字在该前缀下不存在：no_match，消息带上该前缀下可用的技能
    #[test]
    fn missing_name_reports_no_match_with_available_names() {
        let available = keys(&["k1/k2/tdd", "k1/k2/db/sql"]);
        let sel = Selector::named(Some("k1/k2".into()), keys(&["ghost"]));
        let err = sel.resolve(&available).unwrap_err();
        assert_eq!(err.error_code(), "no_match");
        let message = err.to_string();
        assert!(message.contains("k1/k2/ghost"), "{message}");
        assert!(message.contains("tdd"), "{message}");
        assert!(message.contains("db/sql"), "{message}");
    }

    /// 名字含斜杠：形状非法，报错教它把分层写进前缀
    #[test]
    fn slash_in_name_points_at_the_prefix() {
        let err = name_shape("a/tdd").unwrap_err();
        assert_eq!(err.error_code(), "invalid_skill_key");
        assert!(err.reason.contains("--prefix"), "{}", err.reason);

        let sel = Selector::named(Some("k1".into()), keys(&["a/tdd"]));
        assert_eq!(
            sel.check().unwrap_err().error_code(),
            "invalid_skill_key",
            "坏名字先于盘面判定就被拒"
        );
    }

    /// 大写与下划线都不成其为技能名：名字恒为小写字母、数字与连字符
    #[test]
    fn names_are_lowercase_digits_and_hyphens() {
        for bad in ["TDD", "my_lib", "a b", ".", ".."] {
            let err = name_shape(bad).unwrap_err();
            assert_eq!(err.error_code(), "invalid_skill_key", "{bad}");
        }
        for ok in ["tdd", "v2", "my-lib", "1"] {
            assert!(name_shape(ok).is_ok(), "{ok} 应是合法技能名");
        }
    }

    /// 整套：该前缀下（含更深层）的全部技能，按段边界认前缀
    #[test]
    fn all_takes_every_skill_below_the_prefix() {
        let available = members(
            keys(&["k1/k2/tdd", "k1/k2/db/sql", "k1/k2b/other", "k1/k3/x"]),
            Some("k1/k2"),
        );
        assert_eq!(available, keys(&["k1/k2/db/sql", "k1/k2/tdd"]));
        let sel = Selector::all(Some("k1/k2".into()));
        assert_eq!(sel.resolve(&available).unwrap(), available);
    }

    /// 套的边界按段走：前缀本身不是自己的成员，相邻名（k1/k2b）也不进这套
    #[test]
    fn set_boundary_is_segment_wise() {
        assert!(in_set("k1/k2/tdd", Some("k1/k2")));
        assert!(in_set("k1/k2/db/sql", Some("k1/k2")));
        assert!(!in_set("k1/k2b/tdd", Some("k1/k2")));
        assert!(!in_set("k1/k2", Some("k1/k2")));
        assert!(in_set("demo", None));
    }
}
