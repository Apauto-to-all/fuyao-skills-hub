//! 检索面测试：共享夹具住在本文件，各测试只经 search 公开入口观察行为。
//! 夹具是临时 hub（真 init，白名单 .gitignore 原样生效），
//! 命中集合、键投影、排序、spans、截断与语义开关全部对着公开结果断言。

use std::fs;

use super::*;
use crate::ErrorCode;
use crate::hub;
use crate::paths::HubPaths;

pub(super) fn setup() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    (install, paths)
}

/// 造检索夹具 hub：一个平铺 skill、一个分组 skill、agents 配置，
/// 以及白名单外的 export / logs / .lock / 根级散件（都不该被搜到）
pub(super) fn make_hub(paths: &HubPaths) {
    let alpha = paths.store().join("alpha");
    fs::create_dir_all(&alpha).unwrap();
    fs::write(
        alpha.join("SKILL.md"),
        "---\nname: alpha\ndescription: alpha skill\n---\n\nalpha 正文 needle 内容\n",
    )
    .unwrap();
    fs::write(alpha.join("notes.md"), "line1\nNEEDLE line two\n").unwrap();
    let beta = paths.store().join("group").join("beta");
    fs::create_dir_all(&beta).unwrap();
    fs::write(
        beta.join("SKILL.md"),
        "---\nname: beta\ndescription: beta skill\n---\n\nbeta 中文针 needle 中文\n",
    )
    .unwrap();
    fs::create_dir_all(paths.root().join("agents")).unwrap();
    fs::write(
        paths.root().join("agents").join("claude.toml"),
        "model = \"needle-model\"\n",
    )
    .unwrap();
    let export = paths.root().join("export").join("alpha");
    fs::create_dir_all(&export).unwrap();
    fs::write(export.join("SKILL.md"), "export needle\n").unwrap();
    let logs = paths.root().join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("fsh.log"), "log needle\n").unwrap();
    fs::write(paths.root().join(".lock"), "lock needle\n").unwrap();
    fs::write(paths.root().join("stray.txt"), "stray needle\n").unwrap();
}

/// 命中集合只含白名单可见文件，键投影正确，恒定排序，total 精确
#[test]
fn search_covers_whitelist_hits_with_keys_and_stable_order() {
    let (install, paths) = setup();
    make_hub(&paths);
    let res = search(install.path(), "needle", &SearchOptions::default()).unwrap();
    let rows: Vec<(&str, u64, Option<&str>)> = res
        .hits
        .iter()
        .map(|h| (h.path.as_str(), h.line, h.key.as_deref()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("agents/claude.toml", 1, None),
            ("store/alpha/SKILL.md", 6, Some("alpha")),
            ("store/alpha/notes.md", 2, Some("alpha")),
            ("store/group/beta/SKILL.md", 6, Some("group/beta")),
        ]
    );
    assert_eq!(res.total, 4);
    assert!(!res.truncated);
}

/// spans 是行内字符偏移：中文行里按 Unicode 标量值数，不是字节数
#[test]
fn spans_are_char_offsets_not_bytes() {
    let (install, paths) = setup();
    make_hub(&paths);
    let res = search(install.path(), "needle", &SearchOptions::default()).unwrap();
    let hit = res
        .hits
        .iter()
        .find(|h| h.path == "store/group/beta/SKILL.md")
        .unwrap();
    assert_eq!(hit.text, "beta 中文针 needle 中文");
    assert_eq!(hit.spans, vec![(9, 15)]);
}

/// limit 截断只裁 hits，total 始终精确并标 truncated；自定义 limit 生效
#[test]
fn limit_truncates_hits_but_total_stays_exact() {
    let (_install, paths) = setup();
    let many = paths.store().join("many");
    fs::create_dir_all(&many).unwrap();
    let body: String = (0..205).map(|i| format!("{i} needle\n")).collect();
    fs::write(many.join("lines.md"), body).unwrap();

    let res = search(paths.root(), "needle", &SearchOptions::default()).unwrap();
    assert_eq!(res.total, 205);
    assert_eq!(res.hits.len(), 200);
    assert!(res.truncated);
    assert_eq!(res.hits.last().unwrap().line, 200);

    let small = SearchOptions {
        limit: Some(5),
        ..SearchOptions::default()
    };
    let res = search(paths.root(), "needle", &small).unwrap();
    assert_eq!(res.hits.len(), 5);
    assert_eq!(res.total, 205);
    assert!(res.truncated);
}

/// 范围限定收窄检索根（输出路径仍以 hub 根为基准）；
/// 越界、绝对路径、反斜杠、盘符、不存在的范围与白名单外子树一律拒绝
#[test]
fn path_scope_narrows_root_and_rejects_escape() {
    let (install, paths) = setup();
    make_hub(&paths);
    let opts = SearchOptions {
        path: Some("store/alpha".into()),
        ..SearchOptions::default()
    };
    let res = search(install.path(), "needle", &opts).unwrap();
    let rows: Vec<&str> = res.hits.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(rows, vec!["store/alpha/SKILL.md", "store/alpha/notes.md"]);

    for bad in [
        "..",
        "store/../../out",
        "C:/tmp",
        "store\\alpha",
        "no-such-dir",
        "",
        "export",
        "logs",
        ".git",
    ] {
        let opts = SearchOptions {
            path: Some(bad.into()),
            ..SearchOptions::default()
        };
        let err = search(install.path(), "needle", &opts).unwrap_err();
        assert_eq!(
            err.error_code(),
            "search_path_invalid",
            "范围 {bad:?} 未被拒绝"
        );
    }
}

/// smart-case 缺省（全小写忽略大小写、含大写则敏感）、强制忽略大小写、
/// 字面量与整词语义各自生效
#[test]
fn matcher_modes_follow_options() {
    let (install, paths) = setup();
    make_hub(&paths);

    // smart-case：含大写的模式按大小写敏感处理，只命中 notes.md 的 NEEDLE 行
    let upper = search(install.path(), "NEEDLE", &SearchOptions::default()).unwrap();
    assert_eq!(upper.total, 1);
    assert_eq!(upper.hits[0].path, "store/alpha/notes.md");

    // 强制忽略大小写：混合大小写模式也命中全部四行
    let force = SearchOptions {
        case_insensitive: true,
        ..SearchOptions::default()
    };
    let res = search(install.path(), "Needle", &force).unwrap();
    assert_eq!(res.total, 4);

    // 字面量：a.b 只命中字面的点；当正则解释时 axb 也命中
    fs::write(paths.store().join("lit.md"), "a.b dot\naxb cross\n").unwrap();
    let literal = SearchOptions {
        literal: true,
        ..SearchOptions::default()
    };
    let res = search(install.path(), "a.b", &literal).unwrap();
    assert_eq!(res.total, 1);
    let res = search(install.path(), "a.b", &SearchOptions::default()).unwrap();
    assert_eq!(res.total, 2);

    // 整词：need 不再命中 needle
    let word = SearchOptions {
        whole_word: true,
        ..SearchOptions::default()
    };
    let res = search(install.path(), "need", &word).unwrap();
    assert_eq!(res.total, 0);
}

/// glob 只能在白名单可见集之内收窄；负向 glob 放不回被忽略的 export
#[test]
fn globs_narrow_but_cannot_undelete_ignored() {
    let (install, paths) = setup();
    make_hub(&paths);
    let opts = SearchOptions {
        globs: vec!["**/SKILL.md".into()],
        ..SearchOptions::default()
    };
    let res = search(install.path(), "needle", &opts).unwrap();
    let rows: Vec<&str> = res.hits.iter().map(|h| h.path.as_str()).collect();
    assert_eq!(
        rows,
        vec!["store/alpha/SKILL.md", "store/group/beta/SKILL.md"]
    );

    let opts = SearchOptions {
        globs: vec!["!export/**".into()],
        ..SearchOptions::default()
    };
    let res = search(install.path(), "needle", &opts).unwrap();
    assert_eq!(res.total, 4);
    assert!(res.hits.iter().all(|h| !h.path.starts_with("export/")));

    // 全匹配正向 glob 也不能放行被白名单排除的 export、.lock 与根级散件
    let opts = SearchOptions {
        globs: vec!["*".into(), "**/*".into()],
        ..SearchOptions::default()
    };
    let res = search(install.path(), "needle", &opts).unwrap();
    assert_eq!(res.total, 4, "正向 glob 不得穿透白名单");
}

/// 无命中返回空结果；坏正则返回带稳定码的失败
#[test]
fn no_hits_returns_empty_and_bad_pattern_fails_with_code() {
    let (install, paths) = setup();
    make_hub(&paths);
    let res = search(install.path(), "绝不出现的词", &SearchOptions::default()).unwrap();
    assert_eq!(res.total, 0);
    assert!(res.hits.is_empty());
    assert!(!res.truncated);

    let err = search(install.path(), "(unclosed", &SearchOptions::default()).unwrap_err();
    assert_eq!(err.error_code(), "search_pattern_invalid");
}

/// 上下文行参数只进查询面：带不带上下文，命中结构与排序完全一致
#[test]
fn context_lines_leave_hits_and_order_untouched() {
    let (install, paths) = setup();
    make_hub(&paths);
    let plain = search(install.path(), "needle", &SearchOptions::default()).unwrap();
    let ctx = SearchOptions {
        context: Some(1),
        ..SearchOptions::default()
    };
    let with = search(install.path(), "needle", &ctx).unwrap();
    assert_eq!(plain.hits, with.hits);
    assert_eq!(plain.total, with.total);
}
