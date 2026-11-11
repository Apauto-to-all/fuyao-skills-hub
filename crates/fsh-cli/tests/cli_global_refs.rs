//! 引用在只读视图里的可见性：`fsh list` 把项目与全局用法都带进使用清单。

mod common;

use common::{data_of, env_with_metadata_skill, fsh, fsh_env, home_env, hub_root};

#[test]
fn list_carries_global_usage() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    let (out, code) = fsh_env(
        &home_env(&home_s),
        &[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--global",
            "--agent",
            "universal,dsh",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");

    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "{out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let demo = rows
        .iter()
        .find(|r| r["key"] == "demo")
        .expect("demo 在列表");
    let uses = demo["uses"].as_array().unwrap();
    assert_eq!(uses.len(), 1, "全局一条记录: {demo}");
    assert_eq!(uses[0]["scope"], serde_json::json!("global"));
    assert_eq!(uses[0]["agents"], serde_json::json!(["universal", "dsh"]));
    assert!(uses[0]["link"].is_string(), "使用清单带出链接方式: {demo}");
}

#[test]
fn list_carries_project_usage() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let project_s = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    // 项目用法落点直连工作副本
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--project",
        &project_s,
        "--agent",
        "universal",
    ]);
    assert_eq!(code, 0, "use --project: {out}");

    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "{out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let demo = rows
        .iter()
        .find(|r| r["key"] == "demo")
        .expect("demo 在列表");
    let uses = demo["uses"].as_array().unwrap();
    assert_eq!(uses.len(), 1, "项目一条记录: {demo}");
    assert_eq!(uses[0]["scope"], serde_json::json!("project"));
    assert_eq!(uses[0]["agents"], serde_json::json!(["universal"]));
    assert!(uses[0]["project"].is_string(), "项目记录带项目路径: {demo}");
}
