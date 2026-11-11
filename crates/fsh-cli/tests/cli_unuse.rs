//! 撤回的多目标与全局引用判定：真实二进制上的端到端行为。

mod common;

use common::{data_of, env_with_metadata_skill, error_of, fsh, fsh_env, home_env, hub_root};

/// 起一次带临时家目录的 fsh
fn fsh_home(home: &str, args: &[&str]) -> (String, i32) {
    fsh_env(&home_env(home), args)
}

#[test]
fn unuse_global_removes_global_destinations_and_record_only() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    // 全局与项目各用一次
    assert_eq!(
        fsh_home(
            &home_s,
            &[
                "--json",
                "--hub",
                &hub,
                "use",
                "demo",
                "--global",
                "--agent",
                "universal"
            ]
        )
        .1,
        0
    );
    assert_eq!(
        fsh(&[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--agent",
            "universal",
            "--project",
            &proj
        ])
        .1,
        0
    );
    assert!(home.path().join(".agents/skills/demo").exists());

    // 只撤全局：全局落点与全局记录清掉，项目一字不动
    let (out, code) = fsh_home(
        &home_s,
        &["--json", "--hub", &hub, "unuse", "demo", "--global"],
    );
    assert_eq!(code, 0, "unuse --global: {out}");
    let d = data_of(&out);
    let targets = d["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 1, "只给 --global 只有一个目标: {d}");
    assert_eq!(targets[0]["scope"], serde_json::json!("global"));
    assert_eq!(targets[0]["removed"].as_array().unwrap().len(), 1);
    assert!(!home.path().join(".agents/skills/demo").exists());
    assert!(
        project.path().join(".agents/skills/demo").is_dir(),
        "项目落点不得被全局撤回带走"
    );
    let registry = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert!(!registry.contains("[[globals.skills]]"), "{registry}");
    assert!(registry.contains("[[projects."), "{registry}");
}

#[test]
fn unuse_serves_global_and_project_targets_in_one_call() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    // 全局与项目各用一次，再用一次同时撤两个目标
    assert_eq!(
        fsh_home(
            &home_s,
            &[
                "--json",
                "--hub",
                &hub,
                "use",
                "demo",
                "--global",
                "--agent",
                "universal"
            ]
        )
        .1,
        0
    );
    assert_eq!(
        fsh(&[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--agent",
            "universal",
            "--project",
            &proj
        ])
        .1,
        0
    );

    let (out, code) = fsh_home(
        &home_s,
        &[
            "--json",
            "--hub",
            &hub,
            "unuse",
            "demo",
            "--global",
            "--project",
            &proj,
        ],
    );
    assert_eq!(code, 0, "unuse 两目标: {out}");
    let d = data_of(&out);
    let targets = d["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2, "全局 + 一个项目: {d}");
    assert_eq!(targets[0]["scope"], serde_json::json!("global"));
    assert_eq!(targets[1]["scope"], serde_json::json!("project"));
    for target in targets {
        assert_eq!(target["agents"], serde_json::json!(["universal"]));
        assert_eq!(target["removed"].as_array().unwrap().len(), 1);
        assert!(target.get("link").is_some(), "撤回报告带出链接方式");
    }
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
    let (install, _src) = env_with_metadata_skill("1.0.0");
    let hub = hub_root(install.path());
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    let (out, code) = fsh(&["--json", "--hub", &hub, "unuse", "demo"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("no_target"));
    assert!(
        error_of(&out)["message"]
            .as_str()
            .unwrap()
            .contains("--project .")
    );
}

#[test]
fn unuse_rejects_the_whole_call_when_one_target_is_not_registered() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = hub_root(install.path());
    let used = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let used_s = used.path().display().to_string();
    let other_s = other.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    assert_eq!(
        fsh(&[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--agent",
            "universal",
            "--project",
            &used_s
        ])
        .1,
        0
    );

    // 第二个目标没有记录：整次拒绝，已登记的那个落点原样保留
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "unuse",
        "demo",
        "--project",
        &used_s,
        "--project",
        &other_s,
    ]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("not_registered"));
    assert!(
        used.path().join(".agents/skills/demo").is_dir(),
        "不合格目标不得导致部分撤回"
    );
}

#[test]
fn archive_and_remove_are_rejected_while_a_global_usage_holds_the_skill() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    assert_eq!(
        fsh_home(
            &home_s,
            &[
                "--json", "--hub", &hub, "use", "demo", "--global", "--agent", "dsh"
            ]
        )
        .1,
        0
    );

    // 只有全局用法也拦住归档与删除，报错点出占用它的 agent
    for cmd in ["archive", "remove"] {
        let (out, code) = fsh(&["--json", "--hub", &hub, cmd, "demo"]);
        assert_eq!(code, 1, "{cmd} 在全局在用时必须拒绝: {out}");
        let err = error_of(&out);
        assert_eq!(err["code"], serde_json::json!("referenced"), "{cmd}: {err}");
        assert!(
            err["message"].as_str().unwrap().contains("dsh"),
            "{cmd} 的报错要点出全局 agent: {err}"
        );
    }
    // 撤回全局后再归档即可通过
    assert_eq!(
        fsh_home(
            &home_s,
            &["--json", "--hub", &hub, "unuse", "demo", "--global"]
        )
        .1,
        0
    );
    assert_eq!(fsh(&["--json", "--hub", &hub, "archive", "demo"]).1, 0);
}
