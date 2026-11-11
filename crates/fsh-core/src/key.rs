//! 唯一键入参的语法判定：规范形只有 `/` 分段。
//! 反斜杠、空段、`.`/`..`、盘符冒号与版本号形状都在这里拒绝；
//! 所有收 `<skill>` 的命令与入库共用同一套规则，报错也只有一种说法。

use thiserror::Error;

use crate::ErrorCode;

/// 键写错时的结构化错误：原键与违规规则都留下，
/// 调用方既能按 `invalid_skill_key` 分支，也能把原因原样转给用户
#[derive(Debug, Error)]
#[error("键非法: {key}（{reason}）")]
pub struct KeyError {
    /// 调用方给的原始键（原样保留，便于对照输入）
    pub key: String,
    /// 踩中的那一条规则
    pub reason: &'static str,
}

impl ErrorCode for KeyError {
    fn error_code(&self) -> &'static str {
        "invalid_skill_key"
    }
}

/// 校验键的语法：`/` 分段的非空路径，段不得是 `.` 或 `..`，
/// 也不得含盘符冒号或形如 `x.y.z`。
/// 反斜杠不做静默归一：多级键只有一种写法，同一个概念只留一条规则。
pub fn validate(key: &str) -> Result<(), KeyError> {
    let reject = |reason: &'static str| KeyError {
        key: key.to_string(),
        reason,
    };
    if key.contains('\\') {
        return Err(reject("多级键用 `/` 分段，不接受反斜杠"));
    }
    if key.split('/').any(|seg| {
        // 段两侧空白不参与判定：Windows 会吞掉目录名的尾随空白
        let seg = seg.trim();
        seg.is_empty()
            || seg == "."
            || seg == ".."
            || seg.contains(':')
            || crate::version::Version::parse(seg).is_some()
    }) {
        return Err(reject(
            "键段不能含空段、`.`/`..`、盘符冒号或形如 x.y.z 的版本号",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合法形：单级、多级、段内可以带连字符或 `v` 前缀
    #[test]
    fn accepts_slash_separated_keys() {
        for ok in ["demo", "mattpocock/skills/tdd", "my-lib", "tools/v2"] {
            assert!(validate(ok).is_ok(), "{ok} 应是合法键");
        }
    }

    /// 两类违规各一条说法：反斜杠、段内规则
    #[test]
    fn rejects_backslash_and_segment_rules() {
        for bad in [
            "",
            "a\\b",
            "mattpocock\\skills\\tdd",
            "a//b",
            "/lead",
            "tail/",
            ".",
            "..",
            "a/./b",
            "a/../b",
            "C:/x",
            "a/C:",
            "1.2.3",
            "a/1.2.3",
        ] {
            let err = validate(bad).unwrap_err();
            assert_eq!(err.error_code(), "invalid_skill_key", "{bad}");
            assert_eq!(err.key, bad, "错误里应保留原样输入");
        }
    }

    /// 反斜杠的报错直接教正确写法，不留给用户猜
    #[test]
    fn backslash_error_names_the_slash_rule() {
        let err = validate("mattpocock\\skills\\tdd").unwrap_err();
        assert!(
            err.reason.contains("多级键用 `/` 分段"),
            "报错须点名正斜杠写法: {}",
            err.reason
        );
    }
}
