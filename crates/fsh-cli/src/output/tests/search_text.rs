//! search 文本塑形的单元测试：平铺 / -l / -c / 上下文展开与视图取参。
//! 上下文行组由 core 展开好，这里只查标记与分隔——不读盘、不起子进程。

use fsh_core::search::{ContextFile, ContextRow, ContextWindow, SearchHit, SearchResult};

use crate::output::search_text::{SearchView, render_search};

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
        context: Vec::new(),
    }
}

fn row(line: u64, is_hit: bool, text: &str) -> ContextRow {
    ContextRow {
        line,
        is_hit,
        text: text.to_string(),
    }
}

/// 平铺形态：命中行按序原样展开
#[test]
fn flat_lines_come_from_hits_in_order() {
    let r = res(vec![
        hit("a.md", 2, "x needle"),
        hit("b/c.md", 5, "y needle"),
    ]);
    let out = render_search(&r, SearchView::Flat);
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
    let out = render_search(&r, SearchView::Files);
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
    let out = render_search(&r, SearchView::Count);
    assert_eq!(out, vec!["a.md:3", "b.md:1"]);
}

/// 上下文形态：命中行带冒号、上下文行带 -:，行号不连续处与文件之间插 --
#[test]
fn context_rows_mark_hits_and_separate_gaps() {
    let mut r = res(vec![hit("a.md", 2, "x needle"), hit("a.md", 9, "y needle")]);
    r.context = vec![
        ContextFile {
            path: "a.md".to_string(),
            rows: vec![
                row(1, false, "head"),
                row(2, true, "x needle"),
                row(9, true, "y needle"),
            ],
        },
        ContextFile {
            path: "b.md".to_string(),
            rows: vec![row(3, false, "note")],
        },
    ];
    let out = render_search(&r, SearchView::Context);
    assert_eq!(
        out,
        vec![
            "a.md:1-:head",
            "a.md:2:x needle",
            "--",
            "a.md:9:y needle",
            "--",
            "b.md:3-:note",
        ]
    );
}

/// 视图取参：-l 优先于 -c，两者都优先于上下文，缺省平铺；
/// -l 与 -c 的互斥由参数层挡下，视图类型里同一种形态只有一个值
#[test]
fn view_follows_flags_with_files_first() {
    let window = Some(ContextWindow {
        before: 1,
        after: 1,
    });
    assert_eq!(SearchView::of(false, false, None), SearchView::Flat);
    assert_eq!(SearchView::of(false, false, window), SearchView::Context);
    assert_eq!(SearchView::of(true, false, window), SearchView::Files);
    assert_eq!(SearchView::of(false, true, window), SearchView::Count);
}
