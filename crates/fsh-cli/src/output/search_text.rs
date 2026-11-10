//! search 的文本塑形：恒定排序的命中集按形态展开成输出行。
//! -l 列命中文件、-c 出每文件计数；-A/-B/-C 的上下文只在文本模式存在，
//! 上下文行从盘上读出、line 后带 `-` 标记，相邻组行号不连续时 `--` 分隔。

use std::collections::BTreeMap;
use std::path::Path;

use fsh_core::search::{SearchHit, SearchResult};

/// search 的文本塑形：恒定排序的命中集按形态展开成输出行。
/// -l 列命中文件、-c 出每文件计数；-A/-B/-C 的上下文只在文本模式存在，
/// 上下文行从盘上读出、line 后带 - 标记，相邻组行号不连续时 -- 分隔；
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

/// 上下文展开：命中已按文件聚组，同文件内先把全部命中行与上下文区间
/// 收进行号有序表（重叠区间自然合并、同一行只出一次，既被上文带出又
/// 自身命中的行按命中行出），再按行号序输出；文件之间与行号不连续处
/// 插 -- 分隔。上下文行只在盘上存在时出（文件边界外不出），命中行文本
/// 取命中本身——检索已成功，展示层不把命令打失败
fn render_with_context(
    hub_root: &Path,
    res: &SearchResult,
    before: usize,
    after: usize,
) -> Vec<String> {
    // 文件 -> 命中集合（命中恒按路径升序、行号升序排好）
    let mut files: BTreeMap<&str, Vec<&SearchHit>> = BTreeMap::new();
    for hit in &res.hits {
        files.entry(hit.path.as_str()).or_default().push(hit);
    }
    let mut out: Vec<String> = Vec::new();
    for (path, hits) in &files {
        if !out.is_empty() {
            out.push("--".to_string());
        }
        let disk = read_lines(hub_root, path);
        // 行号 -> 是否命中行：命中行先落 true，上下文行只补空位
        let mut marks: BTreeMap<u64, bool> = BTreeMap::new();
        for hit in hits {
            marks.insert(hit.line, true);
            let lo = hit.line.saturating_sub(before as u64).max(1);
            let hi = hit.line.saturating_add(after as u64);
            for n in lo..=hi {
                if disk.contains_key(&n) {
                    marks.entry(n).or_insert(false);
                }
            }
        }
        let mut prev: Option<u64> = None;
        for (&n, &is_hit) in &marks {
            if prev.is_some_and(|p| n > p + 1) {
                out.push("--".to_string());
            }
            let text = if is_hit {
                hits.iter().find(|h| h.line == n).unwrap().text.as_str()
            } else {
                disk.get(&n).map(String::as_str).unwrap_or("")
            };
            let mark = if is_hit { ":" } else { "-:" };
            out.push(format!("{path}:{n}{mark}{text}"));
            prev = Some(n);
        }
    }
    out
}

/// 读文件全部行（1 起编号，去行终止符）。读失败记 WARN 后返回空表：
/// 展示层按「该文件上下文行缺席」降级，命令本身仍成功
fn read_lines(hub_root: &Path, path: &str) -> BTreeMap<u64, String> {
    let abs = hub_root.join(path);
    match std::fs::read(&abs) {
        Ok(bytes) => String::from_utf8_lossy(&bytes)
            .lines()
            .enumerate()
            .map(|(i, l)| (i as u64 + 1, l.to_string()))
            .collect(),
        Err(e) => {
            tracing::warn!(path = %abs.display(), cause = %e, "上下文行读盘降级");
            BTreeMap::new()
        }
    }
}
