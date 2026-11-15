//! 上下文行展开：命中集到「文件 → 行号有序的展示行」的唯一实现。
//! 检索请求上下文窗口时从这里取行组，规则只有一份：读盘失败按缺席降级
//! （检索结果不放大上下文失败）、前后区间重叠合并、同一行只出一次且被
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

/// 上下文窗口：命中行前后各取几行
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContextWindow {
    /// 命中行前取几行
    pub before: usize,
    /// 命中行后取几行
    pub after: usize,
}

impl ContextWindow {
    /// 从三个互斥的参数取窗口：`-C` 同时给前后，`-A`/`-B` 各给一侧；
    /// 零窗口与不给上下文等价，不展开
    pub fn from_flags(
        after: Option<usize>,
        before: Option<usize>,
        context: Option<usize>,
    ) -> Option<Self> {
        let window = match (after, before, context) {
            (_, _, Some(n)) => Self {
                before: n,
                after: n,
            },
            (Some(n), None, None) => Self {
                before: 0,
                after: n,
            },
            (None, Some(n), None) => Self {
                before: n,
                after: 0,
            },
            _ => return None,
        };
        (window.before > 0 || window.after > 0).then_some(window)
    }
}

/// 一个文件的上下文行组：路径与行号升序的展示行
#[derive(Debug, Clone, PartialEq)]
pub struct ContextFile {
    /// 相对 hub 根的路径（正斜杠）
    pub path: String,
    /// 展示行：命中行与它带出的上下文行
    pub rows: Vec<ContextRow>,
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
) -> Vec<ContextFile> {
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
        out.push(ContextFile {
            path: path.to_string(),
            rows,
        });
    }
    out
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
