//! 恢复批量：选择器形状（`--prefix` + `--skill` / `--all`）、工作副本面已占用时
//! 跳过并报告、统一结果载荷与退出码、预演逐条列出动作与跳过项。
//! 全部经真实子进程与盘面观察。

mod common;

use std::fs;
use std::path::Path;

use common::{Run, data_of, error_of, init_hub, json, text};

/// 造一个源技能目录并入库到指定前缀
fn add_skill(hub: &str, src_root: &Path, name: &str, prefix: Option<&str>) {
    let dir = src_root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} 的说明\n---\n\n正文\n"),
    )
    .unwrap();
    let dir = dir.display().to_string();
    let mut args = vec!["add", dir.as_str()];
    if let Some(prefix) = prefix {
        args.extend(["--prefix", prefix]);
    }
    let Run { stdout, code, .. } = json(hub, &args);
    assert_eq!(code, 0, "add {name}: {stdout}");
}

/// 归档一个具名技能：夹具准备用，把工作副本先搬进归档面
fn archive_skill(hub: &str, prefix: Option<&str>, name: &str) {
    let mut args = vec!["archive"];
    if let Some(prefix) = prefix {
        args.extend(["--prefix", prefix]);
    }
    args.extend(["--skill", name]);
    let Run { stdout, code, .. } = json(hub, &args);
    assert_eq!(code, 0, "archive {name}: {stdout}");
}

/// 一套 k1/k2 的 hub：三个技能都躺在归档面里，供批量恢复用例共用
fn hub_with_an_archived_set() -> (tempfile::TempDir, String, tempfile::TempDir) {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    for name in ["tdd", "db", "ops"] {
        add_skill(&hub, src_root.path(), name, Some("k1/k2"));
        archive_skill(&hub, Some("k1/k2"), name);
    }
    (install, hub, src_root)
}

/// 具名一个技能：前缀与末段两段拼出键，归档副本整目录移回工作副本面
#[test]
fn restore_takes_a_named_skill_from_its_prefix() {
    let (install, hub, _src) = hub_with_an_archived_set();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "--prefix", "k1/k2", "--skill", "tdd"]);
    assert_eq!(code, 0, "restore: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!(["k1/k2/tdd"]), "{out}");
    assert_eq!(d["skipped"], serde_json::json!([]), "{out}");
    assert!(install.path().join("store/k1/k2/tdd/SKILL.md").is_file());
    assert!(!install.path().join("archive/k1/k2/tdd").exists());
    assert!(
        install.path().join("archive/k1/k2/db").is_dir(),
        "没点名的技能不动"
    );
}

/// 整套：该前缀下按段边界的全部技能，含更深层；相邻前缀与别套都不进。
/// 整批只产生一条自动提交，消息含整套名与条数
#[test]
fn restore_all_takes_the_whole_set_including_deeper_layers() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    add_skill(&hub, src_root.path(), "tdd", Some("k1/k2"));
    add_skill(&hub, src_root.path(), "sql", Some("k1/k2/db"));
    add_skill(&hub, src_root.path(), "other", Some("k1/k2b"));
    add_skill(&hub, src_root.path(), "far", Some("k1/k3"));
    for (prefix, name) in [
        (Some("k1/k2"), "tdd"),
        (Some("k1/k2/db"), "sql"),
        (Some("k1/k2b"), "other"),
    ] {
        archive_skill(&hub, prefix, name);
    }
    archive_skill(&hub, Some("k1/k3"), "far");
    let before = commit_subjects(&hub);

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 0, "restore --all: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["k1/k2/db/sql", "k1/k2/tdd"]),
        "整套按序恢复，含更深层: {out}"
    );
    assert!(install.path().join("store/k1/k2/tdd").is_dir());
    assert!(install.path().join("store/k1/k2/db/sql").is_dir());
    assert!(
        install.path().join("archive/k1/k2b/other").is_dir(),
        "段边界之外的相邻前缀不进这套"
    );
    assert!(
        install.path().join("archive/k1/k3/far").is_dir(),
        "别套不动"
    );

    // 整批一条提交：预演与实跑的消息形状一致
    let produced = &commit_subjects(&hub)[before.len()..];
    assert_eq!(
        produced,
        ["restore(store): k1/k2 2 个技能"],
        "一次批量只留一条提交"
    );
}

/// 省略前缀即根：键就是技能名自己
#[test]
fn restore_without_prefix_acts_on_root_skills() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    add_skill(&hub, src_root.path(), "demo", None);
    archive_skill(&hub, None, "demo");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "--skill", "demo"]);
    assert_eq!(code, 0, "restore: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["demo"]),
        "{out}"
    );
    assert!(install.path().join("store/demo/SKILL.md").is_file());
}

/// 工作副本面已有同键：跳过并报告，其余照常恢复；一条都没做成才退出码 1
#[test]
fn restore_skips_keys_already_in_the_working_copy() {
    let (install, hub, _src) = hub_with_an_archived_set();
    // 归档面里躺着 db 的同时，工作副本面又出现同名目录：恢复时跳过这一条
    let work = install.path().join("store/k1/k2/db");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 0, "做成了就不算失败: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/k2/ops", "k1/k2/tdd"]),
        "{out}"
    );
    assert_eq!(
        d["skipped"][0]["key"],
        serde_json::json!("k1/k2/db"),
        "{out}"
    );
    assert_eq!(
        d["skipped"][0]["code"],
        serde_json::json!("conflict"),
        "{out}"
    );
    assert!(
        d["skipped"][0]["message"]
            .as_str()
            .unwrap()
            .contains("已占用"),
        "跳过项带原因: {out}"
    );
    assert_eq!(
        fs::read_to_string(work.join("SKILL.md")).unwrap(),
        "# 占位\n",
        "跳过时工作副本原样保留"
    );
    assert!(
        install.path().join("archive/k1/k2/db").is_dir(),
        "跳过时归档副本留在原处"
    );

    // 归档面只剩被占用的那条：一条都做不成，退出码 1
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 1, "只有跳过项时退出码 1: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!([]), "{out}");
    assert_eq!(d["skipped"].as_array().unwrap().len(), 1, "{out}");
}

/// 名字不在归档面：no_match（未归档的活跃技能不可恢复），消息列出可用技能
#[test]
fn restore_unknown_name_reports_no_match_with_available_names() {
    let (_install, hub, _src) = hub_with_an_archived_set();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "--prefix", "k1/k2", "--skill", "ghost"]);
    assert_eq!(code, 1, "{out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("no_match"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("k1/k2/ghost"), "{message}");
    assert!(
        message.contains("tdd") && message.contains("db"),
        "可用技能随行: {message}"
    );
}

/// 用法错误：`--all` 必须有前缀，且不能与 `--skill` 同给
#[test]
fn restore_all_needs_a_prefix_and_excludes_skill() {
    let (_install, hub, _src) = hub_with_an_archived_set();
    for args in [
        // 缺 --prefix：clap 的 requires 挡下
        vec!["restore", "--all"],
        // 空白前缀视同未给：执行边界按同一口径拒绝
        vec!["restore", "--prefix", "  ", "--all"],
        // 两把选择器互斥：clap 挡下
        vec!["restore", "--prefix", "k1", "--all", "--skill", "x"],
    ] {
        let Run {
            stdout: out, code, ..
        } = json(&hub, &args);
        assert_eq!(code, 1, "{args:?} 应按用法错误退出: {out}");
        assert_eq!(
            error_of(&out)["code"],
            serde_json::json!("usage"),
            "{args:?}: {out}"
        );
    }
}

/// 预演：逐条列出动作与跳过项，盘面一字未动
#[test]
fn restore_dry_run_lists_actions_and_skips_without_landing() {
    let (install, hub, _src) = hub_with_an_archived_set();
    let work = install.path().join("store/k1/k2/db");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();
    let before = commit_subjects(&hub);

    // JSON 预演：动作清单与跳过清单都在
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["restore", "--prefix", "k1/k2", "--all", "--dry-run"],
    );
    assert_eq!(code, 0, "预演: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true), "{out}");
    let actions = d["actions"].as_array().unwrap();
    assert!(
        actions.iter().any(|a| a["action"] == "move"
            && a["from"] == "archive/k1/k2/tdd"
            && a["to"] == "store/k1/k2/tdd"),
        "预演列移动步: {actions:?}"
    );
    assert!(
        actions.iter().any(|a| a["action"] == "commit"),
        "预演列提交步: {actions:?}"
    );
    assert_eq!(d["skipped"][0]["code"], serde_json::json!("conflict"));

    // 文本预演：动作与跳过项逐行给出
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["restore", "--prefix", "k1/k2", "--all", "-n"]);
    assert_eq!(code, 0, "预演文本: {out}");
    assert!(
        out.contains("- move archive/k1/k2/tdd -> store/k1/k2/tdd"),
        "{out}"
    );
    assert!(
        out.contains("- commit: restore(store): k1/k2 2 个技能"),
        "{out}"
    );
    assert!(out.contains("- 跳过 k1/k2/db（conflict）"), "{out}");

    // 盘面零变化：归档副本还在，没有新提交
    assert!(install.path().join("archive/k1/k2/tdd/SKILL.md").is_file());
    assert!(!install.path().join("store/k1/k2/tdd").exists());
    assert_eq!(commit_subjects(&hub), before, "预演不产生提交");
}

/// 预演不取写锁：别人正持锁时预演照常给清单，实跑才被锁挡下
#[test]
fn restore_dry_run_does_not_take_the_write_lock() {
    let (install, hub, _src) = hub_with_an_archived_set();
    let paths = fsh_core::paths::HubPaths::from_root(install.path());
    let _held = fsh_core::lock::write_lock(&paths).expect("测试进程应拿到写锁");

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "restore",
            "--prefix",
            "k1/k2",
            "--skill",
            "tdd",
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "预演不取写锁: {out}");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "--prefix", "k1/k2", "--skill", "tdd"]);
    assert_eq!(code, 1, "实跑被写锁挡下: {out}");
    assert!(
        install.path().join("archive/k1/k2/tdd").is_dir(),
        "被锁挡下时归档副本原样保留"
    );
}

/// hub 仓库的机器提交消息，从旧到新
fn commit_subjects(hub: &str) -> Vec<String> {
    let Ok(repo) = fsh_core::git::open_hub(Path::new(hub)) else {
        return Vec::new();
    };
    let mut walk = repo.revwalk().unwrap();
    walk.push_head().unwrap();
    let mut subjects: Vec<String> = walk
        .map(|oid| {
            repo.find_commit(oid.unwrap())
                .unwrap()
                .message()
                .unwrap_or_default()
                .trim_end()
                .to_string()
        })
        .collect();
    subjects.reverse();
    subjects
}
