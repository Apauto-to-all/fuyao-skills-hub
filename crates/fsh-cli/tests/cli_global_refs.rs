//! 全局用法在只读视图里的可见性：`fsh show` 列出占用它的 agent，`fsh gc` 不误删其导出目录。

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
        d["global_agents"],
        serde_json::json!(["dsh", "universal"]),
        "show 要列出全局装着它的 agent: {d}"
    );
    assert_eq!(d["referenced_by"], serde_json::json!([]));

    let (out, code) = fsh(&["--json", "--hub", &hub, "gc"]);
    assert_eq!(code, 0, "gc: {out}");
    assert!(
        install.path().join("export/demo/1.0.0").exists(),
        "全局在用版本不得被回收"
    );
}
