//! 引用命名单一事实源：全库只有这里知道「名字怎么拼」。
//! 唯一键是 store 内相对路径（可多级，如 mattpocock/skills/tdd），
//! store 子路径名字的拼装规则集中于此，skill / git 的全部消费方一律经本模块取名。

/// store 子路径（仓库树内子树与动作文案共用）：`store/<键>`
pub(crate) fn store_subpath(key: &str) -> String {
    format!("store/{key}")
}

/// 归档面子路径（动作文案共用）：`archive/<键>`
pub(crate) fn archive_subpath(key: &str) -> String {
    format!("archive/{key}")
}

/// 内置技能的键前缀段：内置技能落在 `store\builtin\` 下
pub(crate) const BUILTIN_PREFIX: &str = "builtin";

/// 内置技能键：`builtin/<目录名>`，目录名即 `skills\` 下一级目录名
pub(crate) fn builtin_key(name: &str) -> String {
    format!("{BUILTIN_PREFIX}/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// store 子路径拼装：名字表驱动
    #[test]
    fn naming_assembles_store_subpath() {
        assert_eq!(&store_subpath("demo"), "store/demo");
        assert_eq!(
            &store_subpath("mattpocock/skills/tdd"),
            "store/mattpocock/skills/tdd"
        );
    }

    /// 内置技能键：前缀与目录名拼装
    #[test]
    fn builtin_key_joins_prefix_and_name() {
        assert_eq!(builtin_key("fuyao-skills-hub"), "builtin/fuyao-skills-hub");
    }
}
