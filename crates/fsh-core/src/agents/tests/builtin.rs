//! 内置名单测试：条目齐全、全局位可移植。

use super::{agent, setup};
use crate::agents::builtin::BUILTIN;
use crate::agents::{AgentSource, list};

#[test]
fn builtin_table_has_full_static_registry() {
    let (_dir, paths) = setup();
    let agents = list(&paths).unwrap();
    assert_eq!(agents.len(), 83, "内置名单条目数变化需同步更新本断言");
    for (key, display_name, skills_dir, global_skills_dir) in [
        (
            "claude-code",
            "Claude Code",
            ".claude/skills",
            "~/.claude/skills",
        ),
        ("codex", "Codex", ".agents/skills", "~/.agents/skills"),
        (
            "codearts-agent",
            "CodeArts Agent",
            ".codeartsdoer/skills",
            "~/.codeartsdoer/skills",
        ),
        (
            "tabnine-cli",
            "Tabnine CLI",
            ".tabnine/agent/skills",
            "~/.tabnine/agent/skills",
        ),
        ("dsh", "DeepSeek Harness", ".dsh/skills", "~/.dsh/skills"),
        ("universal", "通用", ".agents/skills", "~/.agents/skills"),
        (
            "autoclaw",
            "AutoClaw",
            ".openclaw-autoclaw/skills",
            "~/.openclaw-autoclaw/skills",
        ),
        (
            "workbuddy",
            "WorkBuddy",
            ".workbuddy/skills",
            "~/.workbuddy/skills",
        ),
        (
            "workbuddy-overseas",
            "WorkBuddy AI",
            ".workbuddy-ai/skills",
            "~/.workbuddy-ai/skills",
        ),
    ] {
        let got = agent(&agents, key);
        assert_eq!(got.display_name, display_name);
        assert_eq!(got.skills_dir.as_deref(), Some(skills_dir));
        assert_eq!(got.source, AgentSource::BuiltIn);
        assert_eq!(
            got.global_skills_dir.as_deref(),
            Some(global_skills_dir),
            "{key} 的全局目录回显的应是配置原文"
        );
    }
    // 国内版与海外版目录必须不同：合并成一条会让其中一个版本读不到技能
    assert_ne!(
        agent(&agents, "workbuddy").skills_dir,
        agent(&agents, "workbuddy-overseas").skills_dir,
    );
    assert_ne!(
        agent(&agents, "workbuddy").global_skills_dir,
        agent(&agents, "workbuddy-overseas").global_skills_dir,
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
fn builtin_global_dir_expands_from_config_literal() {
    let (_dir, paths) = setup();
    let agents = list(&paths).unwrap();
    let dsh = agent(&agents, "dsh");
    assert_eq!(dsh.global_skills_dir.as_deref(), Some("~/.dsh/skills"));
    // 落点在解析时从原文展开：读模型只带原文，落点必须是绝对路径
    let dir = crate::agents::expand_global("~/.dsh/skills", &dsh.key).unwrap();
    assert!(dir.is_absolute(), "落点必须是绝对路径: {}", dir.display());
    assert!(
        dir.ends_with(".dsh/skills"),
        "落点应落在 .dsh/skills: {}",
        dir.display()
    );
}

#[test]
fn builtin_table_keeps_global_dirs_portable() {
    assert_eq!(BUILTIN.len(), 83, "内置名单条目数变化需同步更新本断言");
    let mut without_global = Vec::new();
    for row in BUILTIN {
        // 与用户 TOML 同一条规则：两个技能位至少给一个
        assert!(
            row.skills_dir.is_some() || row.global_skills_dir.is_some(),
            "{} 至少要给一个技能位",
            row.key
        );
        let Some(global) = row.global_skills_dir else {
            without_global.push(row.key);
            continue;
        };
        assert!(
            global.starts_with("~/"),
            "{} 的全局目录必须是可移植写法，不得写绝对路径: {global}",
            row.key
        );
    }
    assert_eq!(
        without_global,
        vec!["eve", "promptscript"],
        "没有全局位的条目变化需同步更新本断言"
    );
}
