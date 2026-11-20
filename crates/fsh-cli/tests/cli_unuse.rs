//! 撤回的多目标与全局引用判定：真实二进制上的端到端行为。

mod common;

use common::{
    Run, data_of, env_with_source, error_of, home_env, hub_root, hub_with_used_skill, init_hub_at,
    json, json_env,
};

/// 起一次带临时家目录与指定 hub 的 fsh
fn fsh_home(home: &str, hub: &str, args: &[&str]) -> Run {
    json_env(&home_env(home), hub, args)
}

#[test]
fn unuse_global_removes_global_destinations_and_record_only() {
    let used = hub_with_used_skill();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    // 项目已铺，再铺一次全局用法
    assert_eq!(
        fsh_home(
            &home_s,
            &used.hub,
            &["use", "--skill", "demo", "--global", "--agent", "universal"]
        )
        .code,
        0
    );
    assert!(home.path().join(".agents/skills/demo").exists());

    // 只撤全局：全局落点与全局记录清掉，项目一字不动
    let Run {
        stdout: out, code, ..
    } = fsh_home(
        &home_s,
        &used.hub,
        &[
            "unuse",
            "--skill",
            "demo",
            "--agent",
            "universal",
            "--global",
        ],
    );
    assert_eq!(code, 0, "unuse --global: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["demo"]),
        "只撤全局也按选择器载荷报做掉的键: {d}"
    );
    assert_eq!(d["skipped"], serde_json::json!([]), "{d}");
    assert!(!home.path().join(".agents/skills/demo").exists());
    assert!(used.dest_agents.is_dir(), "项目落点不得被全局撤回带走");
    let registry = std::fs::read_to_string(used.install.path().join("projects.toml")).unwrap();
    assert!(!registry.contains("[[globals.skills]]"), "{registry}");
    assert!(registry.contains("[[projects."), "{registry}");
}

/// 全局与项目目标一次撤回：项目记录须恰好只含被撤的 agent，
/// 终态断言登记簿整段撤净
#[test]
fn unuse_serves_global_and_project_targets_in_one_call() {
    let (install, src) = env_with_source();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);
    // 全局与项目各用一次，再用一次同时撤两个目标
    assert_eq!(
        fsh_home(
            &home_s,
            &hub,
            &["use", "--skill", "demo", "--global", "--agent", "universal"]
        )
        .code,
        0
    );
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "--skill",
                "demo",
                "--agent",
                "universal",
                "--project",
                &proj
            ]
        )
        .code,
        0
    );

    let Run {
        stdout: out, code, ..
    } = fsh_home(
        &home_s,
        &hub,
        &[
            "unuse",
            "--skill",
            "demo",
            "--agent",
            "universal",
            "--global",
            "--project",
            &proj,
        ],
    );
    assert_eq!(code, 0, "unuse 两目标: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["demo"]),
        "一次调用撤两个目标，做掉的键只报一次: {d}"
    );
    assert_eq!(d["skipped"], serde_json::json!([]), "{d}");
    assert!(!home.path().join(".agents/skills/demo").exists());
    assert!(!project.path().join(".agents/skills/demo").exists());
    // 两段记录都撤净：登记簿只剩空文件
    let registry =
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap_or_default();
    assert!(!registry.contains("[[globals.skills]]"), "{registry}");
    assert!(!registry.contains("[[projects."), "{registry}");
}

#[test]
fn unuse_without_any_target_is_rejected() {
    let (install, _src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["unuse", "--skill", "demo", "--agent", "universal"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("no_target"));
    assert!(
        error_of(&out)["message"]
            .as_str()
            .unwrap()
            .contains("--project .")
    );
}

/// 一个键的目标不全：整个键按既有规则不动盘，进 skipped；其余键照常撤。
#[test]
fn unuse_skips_a_skill_when_one_target_is_not_registered() {
    let used = hub_with_used_skill();
    let other = tempfile::tempdir().unwrap();
    let other_s = other.path().display().to_string();

    // 第二个目标没有记录：这个键整键跳过，已登记的落点原样保留
    let Run {
        stdout: out, code, ..
    } = json(
        &used.hub,
        &[
            "unuse",
            "--skill",
            "demo",
            "--agent",
            "universal",
            "--project",
            &used.proj,
            "--project",
            &other_s,
        ],
    );
    assert_eq!(code, 1, "一条都没做成: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!([]), "{d}");
    assert_eq!(d["skipped"][0]["key"], serde_json::json!("demo"), "{d}");
    assert_eq!(
        d["skipped"][0]["code"],
        serde_json::json!("not_registered"),
        "{d}"
    );
    assert!(used.dest_agents.is_dir(), "不合格目标不得导致部分撤回");
}

#[test]
fn archive_skips_and_remove_rejects_while_a_global_usage_holds_the_skill() {
    let (install, src) = env_with_source();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);
    assert_eq!(
        fsh_home(
            &home_s,
            &hub,
            &["use", "--skill", "demo", "--global", "--agent", "dsh"]
        )
        .code,
        0
    );

    // 只有全局用法也拦住删除：跳过清单点名占用它的 agent，退出码 1
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "--skill", "demo"]);
    assert_eq!(code, 1, "remove 在全局在用时必须跳过: {out}");
    let skipped = data_of(&out)["skipped"][0].clone();
    assert_eq!(
        skipped["code"],
        serde_json::json!("referenced"),
        "{skipped}"
    );
    assert!(
        skipped["message"].as_str().unwrap().contains("dsh"),
        "remove 的跳过原因要点出全局 agent: {skipped}"
    );

    // 归档不整笔拒绝：被引用的那条进 skipped，稳定码与原因同失败信封一样可读
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "--skill", "demo"]);
    assert_eq!(code, 1, "一条都没做成: {out}");
    let skipped = data_of(&out)["skipped"][0].clone();
    assert_eq!(skipped["key"], serde_json::json!("demo"), "{out}");
    assert_eq!(skipped["code"], serde_json::json!("referenced"), "{out}");
    assert!(
        skipped["message"].as_str().unwrap().contains("dsh"),
        "归档的跳过原因要点出全局 agent: {skipped}"
    );
    assert!(
        install.path().join("store/demo").is_dir(),
        "跳过时工作副本不动"
    );
    // 撤回全局后再归档即可通过
    assert_eq!(
        fsh_home(
            &home_s,
            &hub,
            &["unuse", "--skill", "demo", "--agent", "dsh", "--global"]
        )
        .code,
        0
    );
    assert_eq!(json(&hub, &["archive", "--skill", "demo"]).code, 0);
}
