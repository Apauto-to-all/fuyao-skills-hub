//! 目标集合与原子性：一次服务全局与多个项目、缺目标报错、任一目标不合格时不动盘。

mod common;

use std::fs;

use common::{
    Run, data_of, env_with_source, error_of, home_env, hub_root, hub_with_used_skill, init_hub_at,
    json, json_env,
};

#[test]
fn use_without_any_target_is_rejected_before_touching_the_hub() {
    let (install, _src) = env_with_source();
    let bare = install.path().join("bare");
    fs::create_dir_all(&bare).unwrap();
    let bare_s = bare.display().to_string();
    let Run {
        stdout: out, code, ..
    } = json(&bare_s, &["use", "demo", "--agent", "universal"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("no_target"));
    let message = error_of(&out)["message"].as_str().unwrap().to_string();
    assert!(
        message.contains("--project ."),
        "报错要点明 --project .: {message}"
    );
}

#[test]
fn one_call_serves_global_and_two_projects_together() {
    let (install, src) = env_with_source();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    let p1 = tempfile::tempdir().unwrap();
    let p2 = tempfile::tempdir().unwrap();
    let p1_s = p1.path().display().to_string();
    let p2_s = p2.path().display().to_string();
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);

    let args = [
        "use",
        "demo",
        "--agent",
        "universal",
        "--global",
        "--project",
        &p1_s,
        "--project",
        &p2_s,
    ];
    let Run {
        stdout: out, code, ..
    } = json_env(&home_env(&home_s), &hub, &args);
    assert_eq!(code, 0, "use 三目标: {out}");
    let d = data_of(&out);
    let targets = d["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 3, "全局一目标 + 两个项目目标: {d}");
    assert_eq!(targets[0]["scope"], serde_json::json!("global"));
    assert_eq!(targets[1]["scope"], serde_json::json!("project"));
    assert_eq!(targets[2]["scope"], serde_json::json!("project"));
    for p in [p1.path(), p2.path()] {
        assert!(
            p.join(".agents/skills/demo/SKILL.md").is_file(),
            "项目落点: {}",
            p.display()
        );
    }
    assert!(home.path().join(".agents/skills/demo/SKILL.md").is_file());
    // 登记簿同时留下全局记录与两条项目记录
    let registry = fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert!(registry.contains("[[globals.skills]]"), "{registry}");
    assert_eq!(registry.matches("[[projects.").count(), 2, "{registry}");
}

#[test]
fn failing_target_leaves_no_destination_and_no_registry_entry() {
    let used = hub_with_used_skill();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let p2 = tempfile::tempdir().unwrap();
    let p2_s = p2.path().display().to_string();
    // 项目里已有一份登记：随后那次调用里它是其中的不合格目标
    let before = fs::read_to_string(used.install.path().join("projects.toml")).unwrap();

    let args = [
        "use",
        "demo",
        "--agent",
        "universal",
        "--global",
        "--project",
        &used.proj,
        "--project",
        &p2_s,
    ];
    let Run {
        stdout: out, code, ..
    } = json_env(&home_env(&home_s), &used.hub, &args);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("already_registered")
    );
    // 整次不动盘：p2 与全局都没有落点，登记簿一字不改
    assert!(!p2.path().join(".agents").exists(), "p2 不该有落点");
    assert!(!home.path().join(".agents").exists(), "全局落点不该建立");
    assert_eq!(
        fs::read_to_string(used.install.path().join("projects.toml")).unwrap(),
        before
    );
}

#[test]
fn dry_run_lists_every_target_and_writes_nothing() {
    let (install, src) = env_with_source();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    let p1 = tempfile::tempdir().unwrap();
    let p2 = tempfile::tempdir().unwrap();
    let p1_s = p1.path().display().to_string();
    let p2_s = p2.path().display().to_string();
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);

    let args = [
        "use",
        "demo",
        "--agent",
        "universal",
        "--global",
        "--project",
        &p1_s,
        "--project",
        &p2_s,
        "--dry-run",
    ];
    let Run {
        stdout: out, code, ..
    } = json_env(&home_env(&home_s), &hub, &args);
    assert_eq!(code, 0, "use --dry-run: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    let targets = d["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 3);
    for target in targets {
        assert_eq!(target["agents"], serde_json::json!(["universal"]));
        assert_eq!(
            target["destinations"].as_array().unwrap().len(),
            1,
            "预演也要给出计划落点: {target}"
        );
        assert!(
            !target["actions"].as_array().unwrap().is_empty(),
            "每个目标都要列出动作: {target}"
        );
    }
    // 零落盘：项目落点、全局落点、登记簿都不出现
    assert!(!install.path().join("projects.toml").exists());
    assert!(!p1.path().join(".agents").exists());
    assert!(!home.path().join(".agents").exists());
}
