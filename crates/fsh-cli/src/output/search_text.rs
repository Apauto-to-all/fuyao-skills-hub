//! search 的文本塑形：恒定排序的命中集按形态展开成输出行。
//! -l 列命中文件、-c 出每文件计数；-A/-B/-C 的上下文行来自 core 的统一
//! 展开（context 模块），行文本后带 `-` 标记，相邻组行号不连续时 `--` 分隔。

use std::path::Path;

use fsh_core::search::{SearchResult, context_rows};

/// 与 --json 同给的分发边界检查已挡下上下文 flag，不会走到这里
pub(crate) fn render_search(
    hub_root: &Path,
    res: &SearchResult,
    files_with_matches: bool,
    count: bool,
    context: (Option<usize>, Option<usize>, Option<usize>),
) -> Vec<String> {
    if files_with_matches {
        // 命中已按路径升序、行号升序排好：文件名只在第一次出现时输出
        let mut last: Option<&str> = None;
        let mut out = Vec::new();
        for h in &res.hits {
            if last != Some(h.path.as_str()) {
                out.push(h.path.clone());
                last = Some(h.path.as_str());
            }
        }
        return out;
    }
    if count {
        // 命中按文件聚在一起：顺序累计，路径变即新起一组
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for h in &res.hits {
            match counts.last_mut() {
                Some((p, n)) if *p == h.path => *n += 1,
                _ => counts.push((h.path.as_str(), 1)),
            }
        }
        return counts
            .into_iter()
            .map(|(p, n)| format!("{p}:{n}"))
            .collect();
    }
    let (before, after) = match context {
        // -C 同时给出前后；-A/-B 互斥由参数解析保证
        (_, _, Some(c)) => (c, c),
        (Some(a), None, None) => (0, a),
        (None, Some(b), None) => (b, 0),
        _ => (0, 0),
    };
    if before == 0 && after == 0 {
        return res
            .hits
            .iter()
            .map(|h| format!("{}:{}:{}", h.path, h.line, h.text))
            .collect();
    }
    render_with_context(hub_root, res, before, after)
}

/// 上下文展开的文本塑形：core 已按文件聚组、区间合并、同一行只出一次，
/// 读盘失败的文件只剩命中行；这里只做标记与分隔——文件之间与行号不连续处
/// 插 -- 分隔，命中行带 `:`、上下文行带 `-:`
fn render_with_context(
    hub_root: &Path,
    res: &SearchResult,
    before: usize,
    after: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (path, rows) in context_rows(hub_root, &res.hits, before, after) {
        if !out.is_empty() {
            out.push("--".to_string());
        }
        let mut prev: Option<u64> = None;
        for row in rows {
            if prev.is_some_and(|p| row.line > p + 1) {
                out.push("--".to_string());
            }
            let mark = if row.is_hit { ":" } else { "-:" };
            out.push(format!("{path}:{}{mark}{}", row.line, row.text));
            prev = Some(row.line);
        }
    }
    out
}
