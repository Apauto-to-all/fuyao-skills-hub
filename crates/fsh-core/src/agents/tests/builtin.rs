//! 内置名单测试：条目齐全、全局位可移植、SKILL.md 的 agent 表不漂移。

use std::collections::BTreeSet;

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

/// 读模型回显配置原文，全局目录在加载时就从原文展开成绝对路径
#[test]
fn builtin_global_dir_is_expanded_at_load() {
    let (_dir, paths) = setup();
    let agents = list(&paths).unwrap();
    let dsh = agent(&agents, "dsh");
    assert_eq!(dsh.global_skills_dir.as_deref(), Some("~/.dsh/skills"));
    let dir = dsh.global_dir.as_ref().expect("dsh 有全局位");
    assert!(
        dir.is_absolute(),
        "全局目录必须是绝对路径: {}",
        dir.display()
    );
    assert!(
        dir.ends_with(".dsh/skills"),
        "全局目录应落在 .dsh/skills: {}",
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

/// 内嵌技能里指定文件的全文：与 init 铺开的同一份内容
fn embedded_skill_file(name: &str, path: &str) -> String {
    let skill = crate::bundled::skills()
        .into_iter()
        .find(|skill| skill.name == name)
        .unwrap_or_else(|| panic!("内置技能表里应有 {name}"));
    let file = skill
        .files
        .into_iter()
        .find(|file| file.path == path && !file.contents.is_empty())
        .unwrap_or_else(|| panic!("{name} 缺 {path}"));
    String::from_utf8(file.contents).expect("内嵌文件应是 UTF-8")
}

/// 内置技能的 agent 表与内置表同源：键与两个技能目录三列逐格对上，
/// 多一行、少一行或某一格写错都算漂移；适配说明一列措辞灵活，不进断言
#[test]
fn skill_doc_lists_every_builtin_agent() {
    let text = embedded_skill_file("fuyao-skills-hub", "references/agents.md");
    let documented = doc_table_rows(&text);
    let builtin: BTreeSet<(String, String, String)> = BUILTIN
        .iter()
        .map(|row| {
            (
                row.key.to_string(),
                row.skills_dir.unwrap_or("—").to_string(),
                row.global_skills_dir.unwrap_or("—").to_string(),
            )
        })
        .collect();
    assert_eq!(
        documented, builtin,
        "agents.md 的 agent 表要与内置表逐格对上"
    );
}

/// agent 表所在文档的标题：表行只在这个区块里抽，防止扫到别的表
const DOC_TABLE_HEADING: &str = "# 内置 agent 配置表";

/// 抽 agent 表：定位标题，跳过表头与分隔行，逐行取
/// 键、项目技能目录、全局技能目录三格（去掉格里的反引号）
fn doc_table_rows(text: &str) -> BTreeSet<(String, String, String)> {
    let body = text
        .split_once(DOC_TABLE_HEADING)
        .unwrap_or_else(|| panic!("agents.md 缺 {DOC_TABLE_HEADING} 区块"))
        .1;
    let mut lines = body.lines().skip_while(|line| !line.starts_with("| ---"));
    assert!(lines.next().is_some(), "agent 表缺分隔行");
    let mut rows = BTreeSet::new();
    for line in lines.take_while(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line
            .split('|')
            .map(|cell| cell.trim().trim_matches('`'))
            .collect();
        rows.insert((
            cells[1].to_string(),
            cells[3].to_string(),
            cells[4].to_string(),
        ));
    }
    assert!(!rows.is_empty(), "agent 表至少要有一行");
    rows
}
