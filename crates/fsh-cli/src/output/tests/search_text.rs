//! search 文本塑形的单元测试：-l / -c / 平铺 / 上下文展开。

use std::path::Path;

use fsh_core::search::{SearchHit, SearchResult};

use crate::output::search_text::render_search;

fn hit(path: &str, line: u64, text: &str) -> SearchHit {
    SearchHit {
        key: None,
        path: path.to_string(),
        line,
        text: text.to_string(),
        spans: Vec::new(),
    }
}

fn res(hits: Vec<SearchHit>) -> SearchResult {
    SearchResult {
        hits,
        total: 0,
        truncated: false,
    }
}

/// 平铺形态：命中行按序原样展开
#[test]
fn flat_lines_come_from_hits_in_order() {
    let r = res(vec![
        hit("a.md", 2, "x needle"),
        hit("b/c.md", 5, "y needle"),
    ]);
    let out = render_search(Path::new("hub"), &r, false, false, (None, None, None));
    assert_eq!(out, vec!["a.md:2:x needle", "b/c.md:5:y needle"]);
}

/// -l 文件去重：同文件多条命中只列一次文件名
#[test]
fn files_with_matches_dedupes_paths() {
    let r = res(vec![
        hit("a.md", 2, "x"),
        hit("a.md", 4, "y"),
        hit("b.md", 1, "z"),
    ]);
    let out = render_search(Path::new("hub"), &r, true, false, (None, None, None));
    assert_eq!(out, vec!["a.md", "b.md"]);
}

/// -c 计数：同文件连续命中累计，路径变化新起一组
#[test]
fn count_groups_by_file_in_order() {
    let r = res(vec![
        hit("a.md", 2, "x"),
        hit("a.md", 4, "y"),
        hit("a.md", 9, "z"),
        hit("b.md", 1, "w"),
    ]);
    let out = render_search(Path::new("hub"), &r, false, true, (None, None, None));
    assert_eq!(out, vec!["a.md:3", "b.md:1"]);
}
