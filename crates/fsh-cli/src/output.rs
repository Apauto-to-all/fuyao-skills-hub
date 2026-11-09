//! 输出塑形：命令名登记与成功 / 失败信封。--json 时 stdout 一行统一信封
//! `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 文本模式动作清单逐行、diff 逐文件输出 unified patch 原文（人读优先）、
//! 其余 pretty JSON。

use std::collections::BTreeMap;
use std::fmt::Display;
use std::path::Path;

use fsh_core::ErrorCode;
use fsh_core::search::{SearchHit, SearchResult};
use serde_json::{Value, json};

use crate::cli::Cmd;

/// 子命令的固定名字：日志里要稳定可查，不用 Debug（会带出参数值与类型括号）
pub(crate) fn cmd_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Init { .. } => "init",
        Cmd::Add { .. } => "add",
        Cmd::List { .. } => "list",
        Cmd::Show { .. } => "show",
        Cmd::Release { .. } => "release",
        Cmd::Archive { .. } => "archive",
        Cmd::Remove { .. } => "remove",
        Cmd::Restore { .. } => "restore",
        Cmd::Agents => "agents",
        Cmd::Projects => "projects",
        Cmd::Use { .. } => "use",
        Cmd::Unuse { .. } => "unuse",
        Cmd::Upgrade { .. } => "upgrade",
        Cmd::Diff { .. } => "diff",
        Cmd::Search { .. } => "search",
        Cmd::Gc { .. } => "gc",
        Cmd::Doctor => "doctor",
    }
}

/// 失败信封载荷：code 取 core 错误的稳定码（ErrorCode 契约，显式声明、
/// 包装变体透传底层真因），message 取 Display 文本
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

/// 任意 core 错误统一收敛为失败信封
pub(crate) fn fail(e: impl Display + ErrorCode) -> Failure {
    Failure {
        code: e.error_code(),
        message: e.to_string(),
    }
}

/// 成功输出：--json 一行信封；文本模式动作清单逐行、diff 逐文件输出
/// unified patch 原文（人读优先）、其余 pretty JSON
pub(crate) fn emit_ok(json: bool, data: &Value) {
    if json {
        println!("{}", json!({"ok": true, "data": data}));
        return;
    }
    if let Some(actions) = data.get("actions").and_then(Value::as_array) {
        for a in actions {
            println!("- {a}");
        }
        return;
    }
    // diff 的文件数组：patch 字段本身就是 unified diff 原文，直接打印
    if let Some(files) = data.get("files").and_then(Value::as_array) {
        for f in files {
            println!("diff {}", f["path"].as_str().unwrap_or(""));
            if let Some(p) = f["patch"].as_str() {
                print!("{p}");
            }
        }
        return;
    }
    // search 的文本形态：path:line:text 平铺、-l 列文件、-c 计数、
    // 上下文组间 -- 分隔（上下文行的 line 后带 - 标记）
    if let Some(lines) = data.get("search_text").and_then(Value::as_array) {
        for line in lines.iter().filter_map(Value::as_str) {
            println!("{line}");
        }
        return;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".to_string())
    );
}

/// 失败输出：--json 一行信封；文本模式进 stderr（不污染 stdout 数据流）
pub(crate) fn emit_err(json: bool, f: &Failure) {
    if json {
        println!(
            "{}",
            json!({"ok": false, "error": {"code": f.code, "message": f.message}})
        );
        return;
    }
    eprintln!("错误 [{}]: {}", f.code, f.message);
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
