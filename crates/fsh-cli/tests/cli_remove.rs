//! 批量删除：选择器形状（`--prefix` + `--skill` / `--all`）、两侧副本一起删、
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

/// 一套 k1/k2 的 hub：三个直接子技能，供批量用例共用
fn hub_with_a_set() -> (tempfile::TempDir, String, tempfile::TempDir) {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    for name in ["tdd", "db", "ops"] {
        add_skill(&hub, src_root.path(), name, Some("k1/k2"));
    }
    (install, hub, src_root)
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

/// 在 store 侧手工放一份工作副本：模拟归档后又出现的同名副本，两侧各一份
fn put_workdir(install: &tempfile::TempDir, key: &str) {
    let work = install.path().join("store").join(key);
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();
}

/// 具名一个技能：前缀与末段两段拼出键，工作副本整目录删掉
#[test]
fn remove_takes_a_named_skill_from_its_prefix() {
    let (install, hub, _src) = hub_with_a_set();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--skill", "tdd"]);
    assert_eq!(code, 0, "remove: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!(["k1/k2/tdd"]), "{out}");
    assert_eq!(d["skipped"], serde_json::json!([]), "{out}");
    assert!(!install.path().join("store/k1/k2/tdd").exists());
    assert!(
        install.path().join("store/k1/k2/db").is_dir(),
        "没点名的技能不动"
    );
}

/// 归档面里的技能也算这套里的候选：只活在归档面的副本同样按名字删得掉
#[test]
fn remove_finds_a_skill_that_only_lives_in_the_archive() {
    let (install, hub, _src) = hub_with_a_set();
    assert_eq!(
        json(&hub, &["archive", "--prefix", "k1/k2", "--skill", "tdd"]).code,
        0,
        "archive 夹具应成功"
    );

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--skill", "tdd"]);
    assert_eq!(code, 0, "remove: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["k1/k2/tdd"]),
        "{out}"
    );
    assert!(!install.path().join("archive/k1/k2/tdd").exists());
    assert_eq!(
        commit_subjects(&hub).last().unwrap(),
        "remove(archive): k1/k2 1 个技能",
        "目标面是归档面"
    );
}

/// 两侧都有副本：一条命令把工作副本与归档副本一起删掉，整批只留一条提交
#[test]
fn remove_deletes_both_faces_in_one_command() {
    let (install, hub, _src) = hub_with_a_set();
    assert_eq!(
        json(&hub, &["archive", "--prefix", "k1/k2", "--skill", "tdd"]).code,
        0,
        "archive 夹具应成功"
    );
    put_workdir(&install, "k1/k2/tdd");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--skill", "tdd"]);
    assert_eq!(code, 0, "remove: {out}");
    assert!(!install.path().join("store/k1/k2/tdd").exists());
    assert!(!install.path().join("archive/k1/k2/tdd").exists());
    let subjects = commit_subjects(&hub);
    let removes: Vec<&String> = subjects
        .iter()
        .filter(|subject| subject.starts_with("remove("))
        .collect();
    assert_eq!(removes.len(), 1, "整批只产生一条删除提交: {subjects:?}");
    assert_eq!(removes[0], "remove(store,archive): k1/k2 1 个技能");
}

/// 整套：该前缀下按段边界的全部技能，含更深层；相邻前缀与别套都不进。
/// 一次批量只产生一条自动提交
#[test]
fn remove_all_takes_the_whole_set_including_deeper_layers() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    add_skill(&hub, src_root.path(), "tdd", Some("k1/k2"));
    add_skill(&hub, src_root.path(), "sql", Some("k1/k2/db"));
    add_skill(&hub, src_root.path(), "other", Some("k1/k2b"));
    add_skill(&hub, src_root.path(), "far", Some("k1/k3"));
    let before = commit_subjects(&hub).len();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 0, "remove --all: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["k1/k2/db/sql", "k1/k2/tdd"]),
        "整套按序删掉，含更深层: {out}"
    );
    assert!(!install.path().join("store/k1/k2/tdd").exists());
    assert!(!install.path().join("store/k1/k2/db/sql").exists());
    assert!(
        install.path().join("store/k1/k2b/other").is_dir(),
        "段边界之外的相邻前缀不进这套"
    );
    assert!(install.path().join("store/k1/k3/far").is_dir(), "别套不动");

    let subjects = commit_subjects(&hub);
    assert_eq!(subjects.len(), before + 1, "一次批量一条提交: {subjects:?}");
    assert_eq!(subjects.last().unwrap(), "remove(store): k1/k2 2 个技能");
}

/// 省略前缀即根：键就是技能名自己
#[test]
fn remove_without_prefix_acts_on_root_skills() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    add_skill(&hub, src_root.path(), "demo", None);

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--skill", "demo"]);
    assert_eq!(code, 0, "remove: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["demo"]),
        "{out}"
    );
    assert!(!install.path().join("store/demo").exists());
}

/// 名字在该前缀下不存在：no_match，消息列出该前缀下可用的技能
#[test]
fn remove_unknown_name_reports_no_match_with_available_names() {
    let (_install, hub, _src) = hub_with_a_set();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--skill", "ghost"]);
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

/// 用法错误：`--all` 必须有前缀，不能与 `--skill` 同给；
/// 旧的整键位置参数不再接受（零兼容）
#[test]
fn remove_all_needs_a_prefix_and_takes_no_positional_key() {
    let (_install, hub, _src) = hub_with_a_set();
    for args in [
        // 缺 --prefix：clap 的 requires 挡下
        vec!["remove", "--all"],
        // 空白前缀视同未给：执行边界按同一口径拒绝
        vec!["remove", "--prefix", "  ", "--all"],
        // 两把选择器互斥：clap 挡下
        vec!["remove", "--prefix", "k1", "--all", "--skill", "x"],
        // 既没点名也没说全要
        vec!["remove", "--prefix", "k1/k2"],
        // 整键位置参数已删：不再有 `fsh remove <键>`
        vec!["remove", "k1/k2/tdd"],
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
fn remove_skips_referenced_skills_and_reports_them() {
    let (install, hub, _src) = hub_with_a_set();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "--prefix",
                "k1/k2",
                "--skill",
                "db",
                "-a",
                "universal",
                "-p",
                &proj
            ]
        )
        .code,
        0,
        "use 夹具应成功"
    );

    // 一套里一条被引用：它进 skipped，其余照做
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--all"]);
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

    // 剩下的这条也被引用时一条都做不成：退出码 1
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 1, "只有跳过项时退出码 1: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!([]), "{out}");
    assert_eq!(d["skipped"].as_array().unwrap().len(), 1, "{out}");
}

/// 预演：逐条列出动作与跳过项，盘上一字未动、不产生提交
#[test]
fn remove_dry_run_lists_actions_and_skips_without_landing() {
    let (install, hub, _src) = hub_with_a_set();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "--prefix",
                "k1/k2",
                "--skill",
                "db",
                "-a",
                "universal",
                "-p",
                &proj
            ]
        )
        .code,
        0
    );
    let before = commit_subjects(&hub).len();

    // JSON 预演：动作清单与跳过清单都在
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--all", "--dry-run"]);
    assert_eq!(code, 0, "预演: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true), "{out}");
    let actions = d["actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a["action"] == "remove" && a["target"] == "store/k1/k2/tdd"),
        "预演列删除步: {actions:?}"
    );
    assert!(
        actions.iter().any(|a| a["action"] == "commit"),
        "预演列提交步: {actions:?}"
    );
    assert_eq!(d["skipped"][0]["code"], serde_json::json!("referenced"));

    // 文本预演：动作与跳过项逐行给出
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["remove", "--prefix", "k1/k2", "--all", "-n"]);
    assert_eq!(code, 0, "预演文本: {out}");
    assert!(out.contains("- remove store/k1/k2/tdd"), "{out}");
    assert!(
        out.contains("- commit: remove(store): k1/k2 2 个技能"),
        "{out}"
    );
    assert!(out.contains("- 跳过 k1/k2/db（referenced）"), "{out}");

    // 盘面零变化：工作副本还在，没有新提交
    assert!(install.path().join("store/k1/k2/tdd/SKILL.md").is_file());
    assert!(
        install.path().join("store/k1/k2/ops").is_dir(),
        "没被点名的技能也留在原处"
    );
    assert_eq!(commit_subjects(&hub).len(), before, "预演不产生提交");
}

/// 预演不取写锁：别人正持锁时预演照常给清单，实跑才被锁挡下
#[test]
fn remove_dry_run_does_not_take_the_write_lock() {
    let (install, hub, _src) = hub_with_a_set();
    let paths = fsh_core::paths::HubPaths::from_root(install.path());
    let _held = fsh_core::lock::write_lock(&paths).expect("测试进程应拿到写锁");

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["remove", "--prefix", "k1/k2", "--skill", "tdd", "--dry-run"],
    );
    assert_eq!(code, 0, "预演不取写锁: {out}");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--prefix", "k1/k2", "--skill", "tdd"]);
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
