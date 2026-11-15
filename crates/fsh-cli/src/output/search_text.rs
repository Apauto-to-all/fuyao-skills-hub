//! search 的文本塑形：恒定排序的命中集按视图展开成输出行。
//! -l 列命中文件、-c 出每文件计数；上下文形态的行组由 core 展开好，
//! 这里只做标记与分隔——命中行带 `:`、上下文行带 `-:`，文件之间与行号
//! 不连续处插 `--`。

use fsh_core::search::{ContextFile, ContextWindow, SearchResult};

/// 文本视图：四种形态互斥，一个值只能是一种
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchView {
    /// 平铺 `path:line:text`
    Flat,
    /// 只列命中文件
    Files,
    /// 每文件命中计数
    Count,
    /// 上下文展开：行组由 core 备好
    Context,
}

impl SearchView {
    /// 从参数取视图：-l 与 -c 的互斥由参数层挡下；-l 优先于 -c，两者都优先
    /// 于上下文，都不给即平铺
    pub(crate) fn of(files_with_matches: bool, count: bool, window: Option<ContextWindow>) -> Self {
        if files_with_matches {
            Self::Files
        } else if count {
            Self::Count
        } else if window.is_some() {
            Self::Context
        } else {
            Self::Flat
        }
    }
}

/// 按视图塑形：命中集与上下文行组都已成序，这里只改形态不改内容
pub(crate) fn render_search(res: &SearchResult, view: SearchView) -> Vec<String> {
    match view {
        SearchView::Files => file_lines(res),
        SearchView::Count => count_lines(res),
        SearchView::Context => context_lines(&res.context),
        SearchView::Flat => res
            .hits
            .iter()
            .map(|h| format!("{}:{}:{}", h.path, h.line, h.text))
            .collect(),
    }
}

/// -l 形态：命中已按路径升序排好，文件名只在第一次出现时输出
fn file_lines(res: &SearchResult) -> Vec<String> {
    let mut last: Option<&str> = None;
    let mut out = Vec::new();
    for h in &res.hits {
        if last != Some(h.path.as_str()) {
            out.push(h.path.clone());
            last = Some(h.path.as_str());
        }
    }
    out
}

/// -c 形态：命中按文件聚在一起，顺序累计，路径变即新起一组
fn count_lines(res: &SearchResult) -> Vec<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for h in &res.hits {
        match counts.last_mut() {
            Some((p, n)) if *p == h.path => *n += 1,
            _ => counts.push((h.path.as_str(), 1)),
        }
    }
    counts
        .into_iter()
        .map(|(p, n)| format!("{p}:{n}"))
        .collect()
}

/// 上下文形态：文件之间与行号不连续处插 -- 分隔
fn context_lines(files: &[ContextFile]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for file in files {
        if !out.is_empty() {
            out.push("--".to_string());
        }
        let mut prev: Option<u64> = None;
        for row in &file.rows {
            if prev.is_some_and(|p| row.line > p + 1) {
                out.push("--".to_string());
            }
            let mark = if row.is_hit { ":" } else { "-:" };
            out.push(format!("{}:{}{mark}{}", file.path, row.line, row.text));
            prev = Some(row.line);
        }
    }
    out
}
