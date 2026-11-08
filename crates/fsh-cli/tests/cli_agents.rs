//! agent 配置面：内置清单、用户配置与稳定错误码。

mod common;

use common::{data_of, env_with_source, error_of, fsh, hub_root};
#[test]
fn agents_lists_builtin_and_user_configs() {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    std::fs::write(
        install.path().join("agents/codex.toml"),
        "display_name = \"My Codex\"\nskills_dir = \"custom/codex\"\n",
    )
    .unwrap();
    std::fs::write(
        install.path().join("agents/my-agent.toml"),
        "skills_dir = \".my-agent/skills\"\n",
    )
    .unwrap();
    // 只给全局目录的 agent：没有项目位，只有全局位
    std::fs::write(
        install.path().join("agents/global-only.toml"),
        "global_skills_dir = \"~/.global-only/skills\"\n",
    )
    .unwrap();

    let (out, code) = fsh(&["--json", "--hub", &hub, "agents"]);
    assert_eq!(code, 0, "agents: {out}");
    let rows = data_of(&out)["agents"].as_array().unwrap().clone();
    let find = |key: &str| {
        rows.iter()
            .find(|r| r["key"] == serde_json::json!(key))
            .unwrap_or_else(|| panic!("缺 agent {key}: {rows:?}"))
    };
    assert_eq!(
        find("dsh")["display_name"],
        serde_json::json!("DeepSeek Harness")
    );
    assert_eq!(find("dsh")["skills_dir"], serde_json::json!(".dsh/skills"));
    assert_eq!(find("dsh")["source"], serde_json::json!("built_in"));
    // 全局目录回显的是配置原文；展开成绝对路径是解析落点时的事
    assert_eq!(
        find("dsh")["global_skills_dir"],
        serde_json::json!("~/.dsh/skills")
    );
    let global_only = find("global-only");
    assert_eq!(
        global_only["global_skills_dir"],
        serde_json::json!("~/.global-only/skills"),
        "只给全局目录时也要回显原文: {global_only:?}"
    );
    assert!(
        global_only.get("skills_dir").is_none(),
        "没有项目目录的 agent 不该写 skills_dir: {global_only:?}"
    );
    assert_eq!(find("codex")["display_name"], serde_json::json!("My Codex"));
    assert_eq!(
        find("codex")["skills_dir"],
        serde_json::json!("custom/codex")
    );
    assert_eq!(find("codex")["source"], serde_json::json!("user"));
    assert_eq!(
        find("my-agent")["display_name"],
        serde_json::json!("my-agent")
    );
    assert_eq!(rows.len(), 85, "83 项内置加 2 项新增");
}

#[test]
fn invalid_agent_config_fails_with_stable_code() {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    std::fs::write(
        install.path().join("agents/bad.toml"),
        "skills_dir = \"../escape\"\n",
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "agents"]);
    assert_eq!(code, 1, "非法配置应失败: {out}");
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("invalid_agent_config")
    );
}
