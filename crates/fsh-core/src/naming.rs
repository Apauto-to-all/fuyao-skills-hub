//! 引用命名单一事实源：全库只有这里知道「名字怎么拼」。
//! 版本 tag、stable 指针、store 子路径三族名字的拼装规则集中于此，
//! skill / deploy / gc / git 的全部消费方一律经本模块取名。

/// 版本 tag 引用名：`<skill>/v<版本>`
pub(crate) fn version_tag(skill: &str, version: &str) -> String {
    format!("{skill}/v{version}")
}

/// 版本 tag 引用名前缀（版本号列举的剥前缀解析用）：`<skill>/v`
pub(crate) fn version_prefix(skill: &str) -> String {
    format!("{skill}/v")
}

/// stable 指针引用名：`<skill>/stable`
pub(crate) fn stable_ref(skill: &str) -> String {
    format!("{skill}/stable")
}

/// store 子路径（仓库树内子树与动作文案共用）：`store/<skill>`
pub(crate) fn store_subpath(skill: &str) -> String {
    format!("store/{skill}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三族名字表驱动：每行 (skill, 版本, 版本引用名, stable 引用名, store 子路径)
    #[test]
    fn naming_assembles_all_three_families() {
        let cases = [
            ("demo", "1.0.0", "demo/v1.0.0", "demo/stable", "store/demo"),
            (
                "my-lib",
                "0.2.13",
                "my-lib/v0.2.13",
                "my-lib/stable",
                "store/my-lib",
            ),
        ];
        for (skill, ver, tag, stable, subpath) in cases {
            assert_eq!(&version_tag(skill, ver), tag);
            assert_eq!(&stable_ref(skill), stable);
            assert_eq!(&store_subpath(skill), subpath);
            // 前缀拼版本号即版本引用名：剥前缀解析与拼装互逆
            assert_eq!(&format!("{}{ver}", version_prefix(skill)), tag);
        }
    }
}
