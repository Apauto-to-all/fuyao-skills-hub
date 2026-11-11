//! 全局作用域：全局落点、全局登记、缺全局位的 agent，以及全局与项目撞同一落点时的回滚。
//! 全局落点一律落在测试自己的家目录，绝不碰真实用户目录。

mod common;

use std::fs;

use common::{data_of, env_with_metadata_skill, error_of, fsh, fsh_env, home_env, hub_root};

#[test]
fn global_use_lands_in_global_dirs_and_registers_one_scope() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(fsh(&["--json", "--hub", &hub, "add", &src_s]).1, 0);

    let args = [
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--global",
        "--agent",
        "universal,dsh",
    ];
    let (out, code) = fsh_env(&home_env(&home_s), &args);
    assert_eq!(code, 0, "use --global: {out}");
    let d = data_of(&out);
    let targets = d["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 1, "只给 --global 只有一个目标: {d}");
    assert_eq!(targets[0]["scope"], serde_json::json!("global"));
    assert!(
        targets[0].get("project").is_none(),
        "全局目标不写 project: {d}"
    );
    assert_eq!(targets[0]["destinations"].as_array().unwrap().len(), 2);
    for name in [".agents/skills/demo", ".dsh/skills/demo"] {
        assert!(
            home.path().join(name).join("SKILL.md").is_file(),
            "全局落点应在测试家目录里: {name}（{d}）"
        );
    }
    // 全局用法进登记簿的全局段，项目段不含它
    let registry = fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert!(registry.contains("[[globals.skills]]"), "{registry}");
    assert!(
        !registry.contains("[projects."),
        "全局用法不进项目段: {registry}"
    );
}

#[test]
fn agent_without_global_dir_is_named_and_nothing_is_written() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj_s = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );

    // eve 没有全局位：给了 --global 就点名它
    let args = [
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--global",
        "--project",
        &proj_s,
        "--agent",
        "eve",
    ];
    let (out, code) = fsh_env(&home_env(&home_s), &args);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("agent_no_global_dir")
    );
    assert!(error_of(&out)["message"].as_str().unwrap().contains("eve"));
    assert!(!install.path().join("projects.toml").exists());
    assert!(!project.path().join(".agents").exists());
}

#[test]
fn mid_run_failure_rolls_back_created_destinations_and_registry() {
    // 项目根就是家目录：全局落点与项目落点是同一个路径。
    // 全局目标排在最前、先建好入口，项目目标随后撞上占用而失败，已建入口与登记都要撤干净
    let (install, src) = env_with_metadata_skill("1.0.0");
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let hub = hub_root(install.path());
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );

    let args = [
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--agent",
        "universal",
        "--global",
        "--project",
        &home_s,
    ];
    let (out, code) = fsh_env(&home_env(&home_s), &args);
    assert_eq!(code, 1, "第二个目标撞上占用应失败: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("occupied"));
    assert!(
        !home.path().join(".agents/skills/demo").exists(),
        "回滚后已建入口不得残留"
    );
    assert!(
        !install.path().join("projects.toml").exists(),
        "回滚后不留登记记录"
    );
}
