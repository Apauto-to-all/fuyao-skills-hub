//! 上下文行展开的单元测试：区间合并、文件边界、重叠去重、读盘降级、
//! 命中行优先与命中上下文填充。

use crate::search::{ContextRow, SearchHit, context_rows, fill_hit_context};

fn hit(path: &str, line: u64, text: &str) -> SearchHit {
    SearchHit {
        key: None,
        path: path.to_string(),
        line,
        text: text.to_string(),
        spans: Vec::new(),
        context: None,
    }
}

/// 同文件两命中的前后文区间重叠：中间行只出一次，命中行按命中行出
#[test]
fn overlapping_windows_merge_and_hits_win() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.md"),
        "l1\nl2 needle\nl3\nl4 needle\nl5\nl6 needle\nl7\n",
    )
    .unwrap();
    let hits = vec![
        hit("m.md", 2, "l2 needle"),
        hit("m.md", 4, "l4 needle"),
        hit("m.md", 6, "l6 needle"),
    ];
    let files = context_rows(dir.path(), &hits, 1, 1);
    assert_eq!(files.len(), 1);
    let (path, rows) = &files[0];
    assert_eq!(path, "m.md");
    let rows: Vec<(u64, bool, &str)> = rows
        .iter()
        .map(|r| (r.line, r.is_hit, r.text.as_str()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (1, false, "l1"),
            (2, true, "l2 needle"),
            (3, false, "l3"),
            (4, true, "l4 needle"),
            (5, false, "l5"),
            (6, true, "l6 needle"),
            (7, false, "l7"),
        ]
    );
}

/// 文件边界：上文不越文件头（行号不小于 1），下文不越文件尾（不存在的行不出）
#[test]
fn context_is_clipped_at_file_edges() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "l1\nl2 needle\nl3\nl4 needle\n").unwrap();
    let hits = vec![hit("a.md", 2, "l2 needle")];
    // -A 5 越过文件尾：只出到第 4 行
    let files = context_rows(dir.path(), &hits, 0, 5);
    let rows: Vec<u64> = files[0].1.iter().map(|r| r.line).collect();
    assert_eq!(rows, vec![2, 3, 4]);
    // -B 5 越过文件头：上文从第 1 行起
    let files = context_rows(dir.path(), &hits, 5, 0);
    let rows: Vec<u64> = files[0].1.iter().map(|r| r.line).collect();
    assert_eq!(rows, vec![1, 2]);
}

/// 多文件分组：文件按路径升序分组输出，各自独立展开
#[test]
fn files_are_grouped_in_path_order() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.md"), "x\ny needle\n").unwrap();
    std::fs::write(dir.path().join("a.md"), "p\nq needle\n").unwrap();
    let hits = vec![hit("b.md", 2, "y needle"), hit("a.md", 2, "q needle")];
    let files = context_rows(dir.path(), &hits, 1, 0);
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, vec!["a.md", "b.md"]);
}

/// 读盘失败降级：文件缺席时只剩命中行，上下文按缺席处理，不报错
#[test]
fn missing_file_keeps_hit_lines_only() {
    let dir = tempfile::tempdir().unwrap();
    let hits = vec![hit("gone.md", 2, "x needle")];
    let files = context_rows(dir.path(), &hits, 1, 1);
    assert_eq!(
        files[0].1,
        vec![ContextRow {
            line: 2,
            is_hit: true,
            text: "x needle".to_string()
        }]
    );
}

/// 命中行文本取命中本身而非盘上重读：展示层不改写检索结果
#[test]
fn hit_text_comes_from_the_hit_not_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("m.md"), "disk text\n").unwrap();
    let hits = vec![hit("m.md", 1, "hit text")];
    let files = context_rows(dir.path(), &hits, 0, 0);
    assert_eq!(files[0].1[0].text, "hit text");
}

/// 空命中集与零上下文：不出任何行组 / 不补上下文行
#[test]
fn empty_hits_and_zero_context() {
    let dir = tempfile::tempdir().unwrap();
    let files = context_rows(dir.path(), &[], 1, 1);
    assert!(files.is_empty());

    std::fs::write(dir.path().join("m.md"), "a\nb\n").unwrap();
    let hits = vec![hit("m.md", 1, "a")];
    let files = context_rows(dir.path(), &hits, 0, 0);
    assert_eq!(files[0].1.len(), 1);
}

/// 命中上下文填充：每个命中都填上紧邻的前后 n 行，命中行自身不出，
/// 相邻命中的区间重叠也不越界取行
#[test]
fn fill_context_attaches_neighbours_to_every_hit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("m.md"),
        "l1\nl2 needle\nl3\nl4 needle\nl5\n",
    )
    .unwrap();
    let mut hits = vec![hit("m.md", 2, "l2 needle"), hit("m.md", 4, "l4 needle")];
    fill_hit_context(dir.path(), &mut hits, 1);
    fn lines(rows: &[crate::search::ContextLine]) -> Vec<(u64, &str)> {
        rows.iter().map(|c| (c.line, c.text.as_str())).collect()
    }
    let first = hits[0].context.as_ref().unwrap();
    assert_eq!(lines(&first.before), vec![(1, "l1")]);
    assert_eq!(lines(&first.after), vec![(3, "l3")]);
    // 第二个命中的前文是第 3 行：它虽落在上一个命中的后文区间内，仍只作前文出
    let second = hits[1].context.as_ref().unwrap();
    assert_eq!(lines(&second.before), vec![(3, "l3")]);
    assert_eq!(lines(&second.after), vec![(5, "l5")]);

    // 文件开头与结尾的命中：越界的一侧为空
    let mut edge = vec![hit("m.md", 1, "l1")];
    fill_hit_context(dir.path(), &mut edge, 1);
    let ctx = edge[0].context.as_ref().unwrap();
    assert!(ctx.before.is_empty());
    assert_eq!(lines(&ctx.after), vec![(2, "l2 needle")]);

    // 读盘失败的文件：上下文缺席但命中照填空上下文，不报错
    let mut gone = vec![hit("gone.md", 2, "x needle")];
    fill_hit_context(dir.path(), &mut gone, 1);
    let ctx = gone[0].context.as_ref().unwrap();
    assert!(ctx.before.is_empty() && ctx.after.is_empty());
}
