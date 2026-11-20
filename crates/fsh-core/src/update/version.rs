//! 版本比较：语义版本规则，不做任何特殊处理。
//! 手写三元组比较会在 `0.10.0` 与 `0.9.0` 上错判，也处理不了预发布版本。

use std::cmp::Ordering;

/// `candidate` 相对 `current` 的先后。解析不了的一侧带出原因：
/// 被动路径据此静默跳过，doctor 据此报「核对不了」而不是盖章成通过
pub fn compare(candidate: &str, current: &str) -> Result<Ordering, String> {
    let candidate =
        parse(candidate).map_err(|e| format!("发布源版本串 {candidate} 解析不了: {e}"))?;
    let current = parse(current).map_err(|e| format!("当前版本 {current} 解析不了: {e}"))?;
    Ok(candidate.cmp(&current))
}

/// `candidate` 是否严格高于 `current`。任一侧解析失败即判否：
/// 解析不了的版本串静默跳过，不提示、不报错
pub fn newer_than(candidate: &str, current: &str) -> bool {
    compare(candidate, current).is_ok_and(|order| order.is_gt())
}

/// 解析版本串：容忍 registry 常见的 `v` 前缀，其余原样交给 semver
fn parse(text: &str) -> Result<semver::Version, semver::Error> {
    semver::Version::parse(text.trim().trim_start_matches('v'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minor_bump_is_newer() {
        assert!(newer_than("0.2.0", "0.1.2"));
    }

    #[test]
    fn double_digit_minor_beats_single_digit() {
        // 手写三元组比较的经典错判点：按字符串比会得到相反结论
        assert!(newer_than("0.10.0", "0.9.0"));
    }

    #[test]
    fn same_version_is_not_newer() {
        assert!(!newer_than("0.1.2", "0.1.2"));
    }

    #[test]
    fn older_version_is_not_newer() {
        assert!(!newer_than("0.1.1", "0.1.2"));
    }

    #[test]
    fn prerelease_does_not_push_you_back_to_an_older_stable() {
        // 正在用预发布版：更旧的稳定版不构成「更新」，不该被劝着倒退
        assert!(!newer_than("0.1.2", "0.2.0-beta.1"));
        // 同一个版本的正式发布照常提示：它确实比预发布版新
        assert!(newer_than("0.2.0", "0.2.0-beta.1"));
        // 更高的稳定版也照常提示
        assert!(newer_than("0.3.0", "0.2.0-beta.1"));
    }

    #[test]
    fn prerelease_of_a_newer_version_counts() {
        assert!(newer_than("0.3.0-beta.1", "0.2.0"));
    }

    #[test]
    fn unparsable_versions_are_skipped() {
        assert!(!newer_than("not-a-version", "0.1.2"));
        assert!(!newer_than("0.1.3", "not-a-version"));
        assert!(!newer_than("", ""));
    }

    #[test]
    fn leading_v_prefix_is_tolerated() {
        assert!(newer_than("v0.1.3", "0.1.2"));
    }

    #[test]
    fn compare_names_the_side_it_could_not_parse() {
        let reason = compare("not-a-version", "0.1.2").unwrap_err();
        assert!(reason.contains("not-a-version"), "{reason}");
        let reason = compare("0.1.3", "not-a-version").unwrap_err();
        assert!(reason.contains("当前版本"), "{reason}");
        assert_eq!(compare("0.1.3", "0.1.2").unwrap(), Ordering::Greater);
        assert_eq!(compare("0.1.2", "0.1.2").unwrap(), Ordering::Equal);
    }
}
