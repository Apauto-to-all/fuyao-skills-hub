//! 上下文行展开：命中集到「文件 → 行号有序的展示行」的唯一实现。
//! 请求前后 n 行的展示层都从这里取行，规则只有一份：读盘失败按缺席降级
//! （检索结果不放大展示层失败）、前后区间重叠合并、同一行只出一次且被
//! 上下文带出的命中行按命中行出。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::engine::SearchHit;

/// 上下文行：行号与行文本（不含行终止符）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextLine {
    /// 行号（1 起）
    pub line: u64,
    /// 行文本
    pub text: String,
}

/// 命中的上下文：命中行前后的行，取自盘上原文件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitContext {
    /// 命中行之前的行，按行号升序，紧邻命中行的在最前
    pub before: Vec<ContextLine>,
    /// 命中行之后的行，按行号升序，紧邻命中行的在最前
    pub after: Vec<ContextLine>,
}

/// 一行展示行：行号、是否命中行与行文本
#[derive(Debug, Clone, PartialEq)]
pub struct ContextRow {
    /// 行号（1 起）
    pub line: u64,
    /// 命中行为 true，纯上下文行为 false
    pub is_hit: bool,
    /// 行文本（不含行终止符）
    pub text: String,
}

/// 把命中集展开成上下文展示行：文件按路径升序分组，组内行号升序；
/// 每个命中行带出前后 n 行，重叠区间合并、同一行只出一次。
/// 命中行文本取命中本身，上下文行取自盘上原文件；读盘失败的文件
/// 只剩命中行（上下文按缺席降级）
pub fn context_rows(
    hub_root: &Path,
    hits: &[SearchHit],
    before: usize,
    after: usize,
) -> Vec<(String, Vec<ContextRow>)> {
    // 文件 -> 命中集合（调用方保证命中恒按路径升序、行号升序，分组后仍有序）
    let mut files: BTreeMap<&str, Vec<&SearchHit>> = BTreeMap::new();
    for hit in hits {
        files.entry(hit.path.as_str()).or_default().push(hit);
    }
    let mut out = Vec::with_capacity(files.len());
    for (path, file_hits) in &files {
        let disk = read_lines(hub_root, path);
        // 行号 -> 是否命中行：命中行先落 true，上下文行只补空位
        let mut marks: BTreeMap<u64, bool> = BTreeMap::new();
        for hit in file_hits.iter() {
            marks.insert(hit.line, true);
            let lo = hit.line.saturating_sub(before as u64).max(1);
            let hi = hit.line.saturating_add(after as u64);
            for n in lo..=hi {
                if disk.contains_key(&n) {
                    marks.entry(n).or_insert(false);
                }
            }
        }
        let rows = marks
            .into_iter()
            .map(|(n, is_hit)| {
                let text = if is_hit {
                    file_hits
                        .iter()
                        .find(|h| h.line == n)
                        .map(|h| h.text.clone())
                        .unwrap_or_default()
                } else {
                    disk.get(&n).cloned().unwrap_or_default()
                };
                ContextRow {
                    line: n,
                    is_hit,
                    text,
                }
            })
            .collect();
        out.push((path.to_string(), rows));
    }
    out
}

/// 把每个命中的前后 n 行上下文填进命中：命中按路径分组后每组只读一次盘，
/// 行取自与展示行同一份展开结果（重叠合并与读盘降级规则只此一份）。
/// 没请求上下文的调用方不调用本函数，命中字段保持 None（序列化时省略）
pub fn fill_hit_context(hub_root: &Path, hits: &mut [SearchHit], n: usize) {
    // 命中集逐条进过分组，每个命中的路径恒有自己的行组
    let rows: BTreeMap<String, Vec<ContextRow>> =
        context_rows(hub_root, hits, n, n).into_iter().collect();
    for hit in hits.iter_mut() {
        hit.context = rows
            .get(&hit.path)
            .map(|rows| hit_context(rows, hit.line, n, n));
    }
}

/// 从展开的展示行里取命中行前后紧邻的 n 行：命中行自身不出，边界外不出
fn hit_context(rows: &[ContextRow], hit_line: u64, before: usize, after: usize) -> HitContext {
    let at = rows.iter().position(|r| r.line == hit_line);
    let pick = |from: usize, to: usize| {
        // 请求行数可能越过展示行末端：截到末行，越界的一侧按缺席处理
        let end = to.min(rows.len());
        rows.get(from..end)
            .map(|ws| {
                ws.iter()
                    .map(|r| ContextLine {
                        line: r.line,
                        text: r.text.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    // 前文：命中行之前的 before 行（含被其它命中的上下文带出的行）
    let before_lines = at
        .map(|i| pick(i.saturating_sub(before), i))
        .unwrap_or_default();
    // 后文：命中行之后的 after 行
    let after_lines = at.map(|i| pick(i + 1, i + 1 + after)).unwrap_or_default();
    HitContext {
        before: before_lines,
        after: after_lines,
    }
}

/// 读文件全部行（1 起编号，去行终止符）。读失败记 WARN 后返回空表，
/// 调用方按缺席降级，不把展示层失败放大成检索失败
fn read_lines(hub_root: &Path, rel: &str) -> BTreeMap<u64, String> {
    let abs = hub_root.join(rel);
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
