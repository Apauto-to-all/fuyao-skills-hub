//! 桥检索测试：库页查询框背后的内容检索。
//! 桥只搬参数与折算错误；语义断言对着与命令同体的核心函数做。

use super::*;
use std::fs;

/// 造一个最小 hub：单个 skill，正文含目标词
fn setup() -> (tempfile::TempDir, HubPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = fsh_core::hub::hub_init(dir.path()).unwrap();
    let alpha = paths.store().join("alpha");
    fs::create_dir_all(&alpha).unwrap();
    fs::write(
        alpha.join("SKILL.md"),
        "---\nname: alpha\ndescription: alpha skill\n---\n\nalpha 正文 needle 内容\nmore\n",
    )
    .unwrap();
    (dir, paths)
}

#[test]
fn search_hub_returns_hits_with_context_and_keys() {
    let (_dir, paths) = setup();
    let res = bridge_search(&paths, "needle", Some(1)).unwrap();
    assert_eq!(res.total, 1);
    let hit = &res.hits[0];
    assert_eq!(hit.key.as_deref(), Some("alpha"));
    assert_eq!(hit.path, "store/alpha/SKILL.md");
    let ctx = hit.context.as_ref().unwrap();
    assert_eq!(ctx.before.len(), 1);
    assert_eq!(ctx.after[0].text, "more");
    // spans 存在且为字符偏移
    assert!(!hit.spans.is_empty());
}

#[test]
fn search_hub_treats_pattern_as_literal() {
    let (_dir, paths) = setup();
    // a.b 含正则元字符：按字面量只命中点号行；按正则解释时 axb 行也会命中（total=2）
    fs::write(
        paths.store().join("alpha").join("lit.md"),
        "a.b dot\naxb cross\n",
    )
    .unwrap();
    let res = bridge_search(&paths, "a.b", None).unwrap();
    assert_eq!(res.total, 1);
    assert_eq!(res.hits[0].path, "store/alpha/lit.md");
}

#[test]
fn search_hub_rejects_blank_query() {
    let (_dir, paths) = setup();
    let err = bridge_search(&paths, "  ", None).unwrap_err();
    assert_eq!(err.code, "usage");
}
