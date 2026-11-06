//! agent 配置测试：内置名单完整性、用户覆盖与非法配置拒绝。

use std::fs;

use super::*;

/// 建一个有 agents 目录的 hub 路径族
fn setup() -> (tempfile::TempDir, HubPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(dir.path());
    fs::create_dir_all(paths.agents()).unwrap();
    (dir, paths)
}

/// 按 key 取一条，不存在即失败
fn agent<'a>(agents: &'a [AgentInfo], key: &str) -> &'a AgentInfo {
    agents.iter().find(|a| a.key == key).unwrap()
}

#[test]
fn builtin_table_has_full_static_registry() {
    let (_dir, paths) = setup();
    let agents = list(&paths).unwrap();
    assert_eq!(agents.len(), 83, "内置名单条目数变化需同步更新本断言");
    for (key, display_name, skills_dir) in [
        ("claude-code", "Claude Code", ".claude/skills"),
        ("codex", "Codex", ".agents/skills"),
        ("codearts-agent", "CodeArts Agent", ".codeartsdoer/skills"),
        ("tabnine-cli", "Tabnine CLI", ".tabnine/agent/skills"),
        ("dsh", "DeepSeek Harness", ".dsh/skills"),
        ("universal", "通用", ".agents/skills"),
        ("autoclaw", "AutoClaw", ".openclaw-autoclaw/skills"),
        ("workbuddy", "WorkBuddy", ".workbuddy/skills"),
        ("workbuddy-overseas", "WorkBuddy AI", ".workbuddy-ai/skills"),
    ] {
        let got = agent(&agents, key);
        assert_eq!(got.display_name, display_name);
        assert_eq!(got.skills_dir, skills_dir);
        assert_eq!(got.source, AgentSource::BuiltIn);
    }
    // 国内版与海外版目录必须不同：合并成一条会让其中一个版本读不到技能
    assert_ne!(
        agent(&agents, "workbuddy").skills_dir,
        agent(&agents, "workbuddy-overseas").skills_dir,
    );
    // 输出按键排序，便于人读与稳定 JSON
    let mut keys: Vec<&str> = agents.iter().map(|a| a.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        agents.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(),
        keys
    );
}

#[test]
fn user_toml_overrides_builtin_as_a_whole() {
    let (_dir, paths) = setup();
    fs::write(
        paths.agents().join("codex.toml"),
        "display_name = \"My Codex\"\nskills_dir = \"custom/codex\"\n",
    )
    .unwrap();
    let agents = list(&paths).unwrap();
    let got = agent(&agents, "codex");
    assert_eq!(got.display_name, "My Codex");
    assert_eq!(got.skills_dir, "custom/codex");
    assert_eq!(got.source, AgentSource::User);
    assert_eq!(agents.len(), 83, "覆盖同名项不得增加条目");
}

#[test]
fn user_toml_adds_agent_and_defaults_display_to_filename() {
    let (_dir, paths) = setup();
    fs::write(
        paths.agents().join("my-agent.toml"),
        "skills_dir = \".my-agent/skills\"\n",
    )
    .unwrap();
    let agents = list(&paths).unwrap();
    let got = agent(&agents, "my-agent");
    assert_eq!(got.display_name, "my-agent");
    assert_eq!(got.skills_dir, ".my-agent/skills");
    assert_eq!(got.source, AgentSource::User);
    assert_eq!(agents.len(), 84);
}

#[test]
fn non_toml_files_are_ignored() {
    let (_dir, paths) = setup();
    fs::write(paths.agents().join("notes.md"), "not an agent").unwrap();
    assert_eq!(list(&paths).unwrap().len(), 83);
}

#[test]
fn invalid_configs_are_rejected_with_stable_code() {
    let cases = [
        ("unknown.toml", "skills_dir = \"ok\"\nnope = 1\n"),
        ("missing.toml", "display_name = \"No Dir\"\n"),
        ("absolute.toml", "skills_dir = \"/abs/skills\"\n"),
        ("parent.toml", "skills_dir = \"../escape\"\n"),
        ("curdir.toml", "skills_dir = \".\"\n"),
        ("empty.toml", "skills_dir = \"\"\n"),
    ];
    for (name, text) in cases {
        let (_dir, paths) = setup();
        fs::write(paths.agents().join(name), text).unwrap();
        let err = list(&paths).unwrap_err();
        assert_eq!(
            err.error_code(),
            "invalid_agent_config",
            "{name} 应报配置错误: {err}"
        );
        assert!(
            err.to_string().contains(name),
            "错误消息应带文件位置: {err}"
        );
    }
}
