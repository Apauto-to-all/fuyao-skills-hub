//! 版本号单一事实源：三段纯数字版本号的解析与比较。

/// 三段纯数字版本号（`major.minor.patch`）。
/// 字段声明顺序即比较顺序：`1.10.0` 大于 `1.9.0`，不会有字符串排序的错位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    /// 主版本
    pub major: u64,
    /// 次版本
    pub minor: u64,
    /// 修订号
    pub patch: u64,
}

impl Version {
    /// 解析 `x.y.z`：必须恰好三段、每段非空且为纯数字，其余形态返回 `None`。
    /// 键段判定与版本段判定共用这一处，两类键因此不会撞形
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        // 第四段存在即不是版本号，如 1.2.3.4
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    /// 解析只认三段纯数字：合法形态给出分量，其余一律 None
    #[test]
    fn parse_accepts_only_three_numeric_segments() {
        assert_eq!(
            Version::parse("1.2.3"),
            Some(Version {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        for bad in [
            "", "1", "1.2", "1.2.3.4", "1.2.x", "1..3", "v1.2.3", " 1.2.3",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad} 不该被当成版本号");
        }
    }

    /// 全序按数字逐段比较：字符串排序会把 1.10.0 排到 1.9.0 前面
    #[test]
    fn ordering_is_numeric_per_segment() {
        let mut list = [
            Version::parse("1.10.0").unwrap(),
            Version::parse("1.9.0").unwrap(),
            Version::parse("2.0.0").unwrap(),
        ];
        list.sort();
        assert_eq!(
            list,
            [
                Version::parse("1.9.0").unwrap(),
                Version::parse("1.10.0").unwrap(),
                Version::parse("2.0.0").unwrap()
            ],
            "排序须按数字段，而非字典序"
        );
    }
}
