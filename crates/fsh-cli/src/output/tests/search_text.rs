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

/// 上下文组内行号不连续补 --；文件边界行不越界
#[test]
fn context_groups_and_file_edges() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.md"),
        "l1
l2 needle
l3
l4 needle
",
    )
    .unwrap();
    let r = res(vec![
        hit("a.md", 2, "l2 needle"),
        hit("a.md", 4, "l4 needle"),
    ]);
    // -B 1：命中 2 的上文 l1 与命中 4 的上文 l3；组间连续（3 跟 4）不分隔
    let out = render_search(dir.path(), &r, false, false, (None, Some(1), None));
    assert_eq!(
        out,
        vec![
            "a.md:1-:l1",
            "a.md:2:l2 needle",
            "a.md:3-:l3",
            "a.md:4:l4 needle",
        ]
    );
    // -A 5 越过文件尾：不存在的行不出
    let r2 = res(vec![hit("a.md", 2, "l2 needle")]);
    let out = render_search(dir.path(), &r2, false, false, (Some(5), None, None));
    assert_eq!(
        out,
        vec!["a.md:2:l2 needle", "a.md:3-:l3", "a.md:4-:l4 needle"]
    );
}

/// 重叠上下文去重：区间重叠的行只出一次，被上文带出的命中行按命中行出
#[test]
fn overlapping_context_prints_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.md"),
        "l1
l2 needle
l3
l4 needle
l5
l6 needle
l7
",
    )
    .unwrap();
    let r = res(vec![
        hit("m.md", 2, "l2 needle"),
        hit("m.md", 4, "l4 needle"),
        hit("m.md", 6, "l6 needle"),
    ]);
    let out = render_search(dir.path(), &r, false, false, (None, None, Some(1)));
    assert_eq!(
        out,
        vec![
            "m.md:1-:l1",
            "m.md:2:l2 needle",
            "m.md:3-:l3",
            "m.md:4:l4 needle",
            "m.md:5-:l5",
            "m.md:6:l6 needle",
            "m.md:7-:l7",
        ]
    );
    // -A 2 时第 4 行落在上一命中的后文区间内，仍按命中行出且只出一次
    let out = render_search(dir.path(), &r, false, false, (Some(2), None, None));
    assert_eq!(
        out,
        vec![
            "m.md:2:l2 needle",
            "m.md:3-:l3",
            "m.md:4:l4 needle",
            "m.md:5-:l5",
            "m.md:6:l6 needle",
            "m.md:7-:l7",
        ]
    );
}

/// 读盘失败的文件：上下文行缺席（降级），命中行照出
#[test]
fn missing_file_drops_context_but_keeps_hits() {
    let dir = tempfile::tempdir().unwrap();
    let r = res(vec![hit("gone.md", 2, "x needle")]);
    let out = render_search(dir.path(), &r, false, false, (None, None, Some(1)));
    assert_eq!(out, vec!["gone.md:2:x needle"]);
}
