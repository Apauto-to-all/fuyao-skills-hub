//! 归档批量：选择器形状（`--prefix` + `--skill` / `--all`）、跳过并报告、
//! 统一结果载荷与退出码、预演逐条列出动作与跳过项。全部经真实子进程与盘面观察。

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

/// 一套 k1/k2 的 hub：three 个直接子技能，供批量用例共用
fn hub_with_a_set() -> (tempfile::TempDir, String, tempfile::TempDir) {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    for name in ["tdd", "db", "ops"] {
        add_skill(&hub, src_root.path(), name, Some("k1/k2"));
    }
    (install, hub, src_root)
}

/// 具名一个技能：前缀与末段两段拼出键，工作副本移入归档面
#[test]
fn archive_takes_a_named_skill_from_its_prefix() {
    let (install, hub, _src) = hub_with_a_set();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "--prefix", "k1/k2", "--skill", "tdd"]);
    assert_eq!(code, 0, "archive: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!(["k1/k2/tdd"]), "{out}");
    assert_eq!(d["skipped"], serde_json::json!([]), "{out}");
    assert!(!install.path().join("store/k1/k2/tdd").exists());
    assert!(install.path().join("archive/k1/k2/tdd/SKILL.md").is_file());
    assert!(
        install.path().join("store/k1/k2/db").is_dir(),
        "没点名的技能不动"
    );
}

/// 整套：该前缀下按段边界的全部技能，含更深层；相邻前缀与别套都不进
#[test]
fn archive_all_takes_the_whole_set_including_deeper_layers() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    add_skill(&hub, src_root.path(), "tdd", Some("k1/k2"));
    add_skill(&hub, src_root.path(), "sql", Some("k1/k2/db"));
    add_skill(&hub, src_root.path(), "other", Some("k1/k2b"));
    add_skill(&hub, src_root.path(), "far", Some("k1/k3"));

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 0, "archive --all: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["k1/k2/db/sql", "k1/k2/tdd"]),
        "整套按序做掉，含更深层: {out}"
    );
    assert!(!install.path().join("store/k1/k2/tdd").exists());
    assert!(!install.path().join("store/k1/k2/db/sql").exists());
    assert!(
        install.path().join("store/k1/k2b/other").is_dir(),
        "段边界之外的相邻前缀不进这套"
    );
    assert!(install.path().join("store/k1/k3/far").is_dir(), "别套不动");
}

/// 省略前缀即根：键就是技能名自己
#[test]
fn archive_without_prefix_acts_on_root_skills() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    add_skill(&hub, src_root.path(), "demo", None);

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "--skill", "demo"]);
    assert_eq!(code, 0, "archive: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["demo"]),
        "{out}"
    );
    assert!(install.path().join("archive/demo/SKILL.md").is_file());
}

/// 技能名形状非法：复用 invalid_skill_key，含斜杠时直接教它写进前缀
#[test]
fn archive_rejects_malformed_skill_names() {
    let (_install, hub, _src) = hub_with_a_set();
    for (name, hint) in [
        ("a/b", "--prefix"),
        ("TDD", "小写字母"),
        ("my_lib", "小写字母"),
    ] {
        let Run {
            stdout: out, code, ..
        } = json(&hub, &["archive", "--prefix", "k1/k2", "--skill", name]);
        assert_eq!(code, 1, "{name} 应被拒: {out}");
        let err = error_of(&out);
        assert_eq!(err["code"], serde_json::json!("invalid_skill_key"), "{err}");
        assert!(
            err["message"].as_str().unwrap().contains(hint),
            "{name} 的报错应指向 {hint}: {err}"
        );
    }
}

/// 名字在该前缀下不存在：no_match，消息列出该前缀下可用的技能
#[test]
fn archive_unknown_name_reports_no_match_with_available_names() {
    let (_install, hub, _src) = hub_with_a_set();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "--prefix", "k1/k2", "--skill", "ghost"]);
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
fn archive_all_needs_a_prefix_and_excludes_skill() {
    let (_install, hub, _src) = hub_with_a_set();
    for args in [
        // 缺 --prefix：clap 的 requires 挡下
        vec!["archive", "--all"],
        // 空白前缀视同未给：执行边界按同一口径拒绝
        vec!["archive", "--prefix", "  ", "--all"],
        // 两把选择器互斥：clap 挡下
        vec!["archive", "--prefix", "k1", "--all", "--skill", "x"],
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

/// 被引用的技能跳过并报告，其余照常；一条都没做成才退出码 1
#[test]
fn archive_skips_referenced_skills_and_reports_them() {
    let (install, hub, _src) = hub_with_a_set();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(&hub, &["use", "k1/k2/db", "-a", "universal", "-p", &proj]).code,
        0,
        "use 夹具应成功"
    );

    // 一套里一条被引用：它进 skipped，其余照做
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "--prefix", "k1/k2", "--all"]);
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
        serde_json::json!("referenced"),
        "{out}"
    );
    assert!(
        d["skipped"][0]["message"]
            .as_str()
            .unwrap()
            .contains("仍被引用"),
        "跳过项带原因: {out}"
    );
    assert!(
        install.path().join("store/k1/k2/db").is_dir(),
        "被引用的留在原处"
    );
    assert!(install.path().join("archive/k1/k2/tdd").is_dir());

    // 剩下的这条也被引用时一条都做不成：退出码 1
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 1, "只有跳过项时退出码 1: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!([]), "{out}");
    assert_eq!(d["skipped"].as_array().unwrap().len(), 1, "{out}");
}

/// 预演：逐条列出动作与跳过项，盘上一字未动、不产生提交
#[test]
fn archive_dry_run_lists_actions_and_skips_without_landing() {
    let (install, hub, _src) = hub_with_a_set();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(&hub, &["use", "k1/k2/db", "-a", "universal", "-p", &proj]).code,
        0
    );

    // JSON 预演：动作清单与跳过清单都在
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["archive", "--prefix", "k1/k2", "--all", "--dry-run"],
    );
    assert_eq!(code, 0, "预演: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true), "{out}");
    let actions = d["actions"].as_array().unwrap();
    assert!(
        actions.iter().any(|a| a["action"] == "move"
            && a["from"] == "store/k1/k2/tdd"
            && a["to"] == "archive/k1/k2/tdd"),
        "预演列移动步: {actions:?}"
    );
    assert!(
        actions.iter().any(|a| a["action"] == "commit"),
        "预演列提交步: {actions:?}"
    );
    assert_eq!(d["skipped"][0]["code"], serde_json::json!("referenced"));

    // 文本预演：动作与跳过项逐行给出
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["archive", "--prefix", "k1/k2", "--all", "-n"]);
    assert_eq!(code, 0, "预演文本: {out}");
    assert!(
        out.contains("- move store/k1/k2/tdd -> archive/k1/k2/tdd"),
        "{out}"
    );
    assert!(
        out.contains("- commit: archive(store): k1/k2 2 个技能"),
        "{out}"
    );
    assert!(out.contains("- 跳过 k1/k2/db（referenced）"), "{out}");

    // 盘面零变化：工作副本还在，没有新提交
    assert!(install.path().join("store/k1/k2/tdd/SKILL.md").is_file());
    assert!(!install.path().join("archive/k1/k2/tdd").exists());
    assert!(
        install.path().join("store/k1/k2/ops").is_dir(),
        "没被点名的技能也留在原处"
    );
}

/// 预演不取写锁：别人正持锁时预演照常给清单，实跑才被锁挡下
#[test]
fn archive_dry_run_does_not_take_the_write_lock() {
    let (install, hub, _src) = hub_with_a_set();
    let paths = fsh_core::paths::HubPaths::from_root(install.path());
    let _held = fsh_core::lock::write_lock(&paths).expect("测试进程应拿到写锁");

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "archive",
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
    } = json(&hub, &["archive", "--prefix", "k1/k2", "--skill", "tdd"]);
    assert_eq!(code, 1, "实跑被写锁挡下: {out}");
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("lock_held"),
        "{out}"
    );
    assert!(
        install.path().join("store/k1/k2/tdd").is_dir(),
        "被锁挡下时不动盘"
    );
}
