//! 引用视图在只读视图里的可见性：`fsh show` 按作用域列出使用记录，`fsh gc` 不误删全局在用的导出目录。

mod common;

use common::{data_of, env_with_versioned_skill, fsh, fsh_env, home_env, hub_root};

#[test]
fn show_lists_agents_holding_a_global_usage_and_gc_keeps_the_export() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    // 固定版本才会产生导出目录：gc 是否误删就看它
    let (out, code) = fsh_env(
        &home_env(&home_s),
        &[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo@1.0.0",
            "--global",
            "--agent",
            "universal,dsh",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");

    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 0, "{out}");
    let d = data_of(&out);
    assert_eq!(
        d["usage"],
        serde_json::json!([{
            "scope": "global",
            "agents": ["universal", "dsh"],
            "version": "1.0.0",
        }]),
        "引用视图一条记录一行，全局记录没有项目并带出登记版本: {d}"
    );
    assert!(
        d.get("referenced_by").is_none() && d.get("global_agents").is_none(),
        "引用视图只出 usage 一块: {d}"
    );

    let (out, code) = fsh(&["--json", "--hub", &hub, "gc"]);
    assert_eq!(code, 0, "gc: {out}");
    assert!(
        install.path().join("export/demo/1.0.0").exists(),
        "全局在用版本不得被回收"
    );
}

#[test]
fn show_lists_a_project_usage_with_its_display_path() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let project_s = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    // 项目用法不给版本即跟最新：记录 version 为空，落点链到工作副本
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

    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 0, "{out}");
    let d = data_of(&out);
    let usage = d["usage"].as_array().cloned().unwrap_or_default();
    assert_eq!(usage.len(), 1, "一条项目记录一行: {d}");
    assert_eq!(usage[0]["scope"], serde_json::json!("project"));
    assert_eq!(
        usage[0]["project"],
        serde_json::json!(project_s),
        "项目路径按展示形态带出: {d}"
    );
    assert_eq!(
        usage[0]["version"],
        serde_json::Value::Null,
        "跟最新时版本如实为空: {d}"
    );
    assert_eq!(usage[0]["agents"], serde_json::json!(["universal"]));
}
