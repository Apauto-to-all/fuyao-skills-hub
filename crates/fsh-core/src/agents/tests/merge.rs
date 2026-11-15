//! 用户覆盖测试：整项覆盖、新增、忽略非 TOML，以及非法配置的拒绝。

use std::fs;

use super::{agent, setup};
use crate::ErrorCode;
use crate::agents::{AgentSource, list};

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
    assert_eq!(got.skills_dir.as_deref(), Some("custom/codex"));
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
    assert_eq!(got.skills_dir.as_deref(), Some(".my-agent/skills"));
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
fn either_dir_alone_or_both_together_are_legal() {
    let (_dir, paths) = setup();
    fs::write(
        paths.agents().join("both.toml"),
        "skills_dir = \".both/skills\"\nglobal_skills_dir = \"~/.both/skills\"\n",
    )
    .unwrap();
    fs::write(
        paths.agents().join("global-only.toml"),
        "global_skills_dir = \"~/.global-only/skills\"\n",
    )
    .unwrap();
    let agents = list(&paths).unwrap();
    let both = agent(&agents, "both");
    assert_eq!(both.skills_dir.as_deref(), Some(".both/skills"));
    assert_eq!(both.global_skills_dir.as_deref(), Some("~/.both/skills"));
    // 全局目录在加载时展开成绝对路径，配置原文只留给回显
    let expanded = both.global_dir.as_ref().expect("有全局位就有展开后的目录");
    assert!(expanded.is_absolute() && expanded.ends_with(".both/skills"));
    let global_only = agent(&agents, "global-only");
    assert_eq!(global_only.skills_dir, None, "只给全局目录时不该有项目目录");
    assert_eq!(
        global_only.global_skills_dir.as_deref(),
        Some("~/.global-only/skills"),
        "全局目录回显的是配置原文"
    );
    // 只给项目目录的一项（内置项）同样合法
    assert!(agent(&agents, "dsh").skills_dir.is_some());
}

#[test]
fn invalid_global_dirs_name_the_agent_and_the_field() {
    let cases = [
        (
            "unset-var.toml",
            "global_skills_dir = \"%FSH_TEST_UNSET_9F3%/skills\"\n",
        ),
        // 覆盖内置项同样不带内置的适配声明：显式写未设置的变量必须报错，不静默回落
        (
            "claude-code.toml",
            "global_skills_dir = \"%FSH_TEST_UNSET_9F3%/skills\"\n",
        ),
        ("relative.toml", "global_skills_dir = \"relative/skills\"\n"),
        ("escape.toml", "global_skills_dir = \"~/../escape\"\n"),
        ("empty-global.toml", "global_skills_dir = \"\"\n"),
        ("no-dir.toml", "display_name = \"No Dir\"\n"),
    ];
    for (name, text) in cases {
        let (_dir, paths) = setup();
        fs::write(paths.agents().join(name), text).unwrap();
        let err = list(&paths).unwrap_err();
        assert_eq!(err.error_code(), "invalid_agent_config", "{name}: {err}");
        let msg = err.to_string();
        assert!(
            msg.contains(name.trim_end_matches(".toml")),
            "错误消息应点名 agent: {msg}"
        );
        assert!(msg.contains("skills_dir"), "错误消息应点名字段: {msg}");
    }
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
