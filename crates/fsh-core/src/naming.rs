//! 引用命名单一事实源：全库只有这里知道「名字怎么拼」。
//! 唯一键是 store 内相对路径（可多级，如 mattpocock/skills/tdd），
//! 版本 tag、store 子路径、导出目录三族名字的拼装规则集中于此，
//! skill / gc / git 的全部消费方一律经本模块取名。

/// 版本 tag 引用名：`<键>/v<版本>`
pub(crate) fn version_tag(key: &str, version: &str) -> String {
    format!("{key}/v{version}")
}

/// store 子路径（仓库树内子树与动作文案共用）：`store/<键>`
pub(crate) fn store_subpath(key: &str) -> String {
    format!("store/{key}")
}

/// 某键的导出目录前缀（`export/<键>/`）：整键清理时的文案与扫描起点
pub(crate) fn exported_key_subpath(key: &str) -> String {
    format!("export/{key}")
}

/// 版本引用解析：尾段是 `v<三段纯数字>` 时给出 (键, 版本)；其余形态不给。
/// 多级键用 rsplit 定界——键自身可含斜杠，尾段语义由引用名规范独占
pub(crate) fn parse_version_tag_ref(tag_ref: &str) -> Option<(&str, &str)> {
    let tag = tag_ref.strip_prefix("refs/tags/")?;
    let (key, last) = tag.rsplit_once('/')?;
    let version = last
        .strip_prefix('v')
        .filter(|v| crate::version::Version::parse(v).is_some())?;
    Some((key, version))
}

/// 从完整 tag 引用名反解唯一键：版本引用形态给出键，其余形态
/// （别键的嵌套子命名空间）返回 None
pub(crate) fn key_from_tag_ref(tag_ref: &str) -> Option<&str> {
    parse_version_tag_ref(tag_ref).map(|(key, _)| key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三族名字表驱动：每行 (键, 版本, 版本引用名, store 子路径, 导出子路径)
    #[test]
    fn naming_assembles_all_three_families() {
        let cases = [
            ("demo", "1.0.0", "demo/v1.0.0", "store/demo"),
            ("my-lib", "0.2.13", "my-lib/v0.2.13", "store/my-lib"),
            // 多级键：斜杠原样进引用名，各级消费方按整体处理
            (
                "mattpocock/skills/tdd",
                "1.2.0",
                "mattpocock/skills/tdd/v1.2.0",
                "store/mattpocock/skills/tdd",
            ),
        ];
        for (key, ver, tag, subpath) in cases {
            assert_eq!(&version_tag(key, ver), tag);
            assert_eq!(&store_subpath(key), subpath);
            // 拼装与解析互逆，导出路径同源
            assert_eq!(
                parse_version_tag_ref(&format!("refs/tags/{tag}")),
                Some((key, ver)),
                "版本引用应解析回 (键, 版本)"
            );
            // 导出目录前缀与键同源，整键清理可直接复用
            assert_eq!(exported_key_subpath(key), format!("export/{key}"));
        }
    }

    /// tag 反解键：版本引用可解，非版本形态不误认
    #[test]
    fn key_from_tag_ref_parses_version_refs() {
        assert_eq!(
            key_from_tag_ref("refs/tags/demo/v1.0.0"),
            Some("demo"),
            "单级版本引用应反解出键"
        );
        assert_eq!(
            key_from_tag_ref("refs/tags/mattpocock/skills/tdd/v0.1.0"),
            Some("mattpocock/skills/tdd"),
            "多级键版本引用应反解出完整键"
        );
        // 键末段以 v 开头也不能误剥：tools/v2 是键的一部分
        assert_eq!(
            key_from_tag_ref("refs/tags/tools/v2/v1.0.0"),
            Some("tools/v2"),
            "键内 v 段不参与版本尾段判定（rsplit 只看最后一段）"
        );
        assert_eq!(key_from_tag_ref("refs/tags/unknown-thing"), None);
        assert_eq!(key_from_tag_ref("refs/tags/a/b/not-a-version"), None);
        // 子键名首字符是 v 也只归它自己：判定看尾段，不看首字符
        assert_eq!(
            key_from_tag_ref("refs/tags/a/b/vendor/v1.0.0"),
            Some("a/b/vendor"),
            "子键命名空间的引用归子键，父键不得染指"
        );
    }
}
