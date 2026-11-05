//! agent 配置：内置静态名单 + hub 内用户 TOML 覆盖。
//!
//! agent 只是数据：键是 `--agent` 取值，`skills_dir` 是相对项目根的技能目录。
//! 内置名单随本程序发布版本更新；用户在 `agents\<键>.toml` 放同名文件即整项
//! 覆盖内置项，放新文件名即新增一个 agent。配置非法一律报错，不回退内置项。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::paths::HubPaths;

/// agent 配置错误
#[derive(Debug, Error)]
pub enum AgentError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// 用户 TOML 无法解析或字段取值非法
    #[error("agent 配置非法: {path}（{reason}）")]
    InvalidConfig { path: String, reason: String },
    /// 请求的 agent 不在内置与用户配置的并集里
    #[error("不认识的 agent: {key}（认得的 agent: {known}）")]
    UnknownAgent { key: String, known: String },
    /// 反查使用记录时登记簿不可读
    #[error("登记簿错误: {0}")]
    Registry(#[from] crate::hub::HubError),
}

impl ErrorCode for AgentError {
    fn error_code(&self) -> &'static str {
        match self {
            AgentError::Io(_) => "io",
            AgentError::InvalidConfig { .. } => "invalid_agent_config",
            AgentError::UnknownAgent { .. } => "unknown_agent",
            AgentError::Registry(error) => error.error_code(),
        }
    }
}

/// agent 配置来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSource {
    /// 随程序发布的内置项
    BuiltIn,
    /// hub 内用户 TOML 覆盖或新增
    User,
}

/// 一个 agent 在某项目里的使用记录（从 hub 侧登记簿反查）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentUsage {
    /// 项目根绝对路径
    pub project: String,
    /// skill 唯一键
    pub key: String,
    /// 固定版本；缺省表示跟随最新
    pub version: Option<String>,
}

/// 一个可用 agent 的公共视图
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentInfo {
    /// agent 键（`--agent` 取值，等于 TOML 文件名去扩展名）
    pub key: String,
    /// 给人看的名字
    pub display_name: String,
    /// 相对项目根的技能目录
    pub skills_dir: String,
    /// 配置来源
    pub source: AgentSource,
    /// 使用记录（list 从登记簿填充；resolve 留空，避免热路径读登记簿）
    pub usages: Vec<AgentUsage>,
}

/// 用户 TOML 的形状；未知字段必须报错，不能静默忽略
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserAgentConfig {
    /// 可选显示名；缺省用文件名
    #[serde(default)]
    display_name: Option<String>,
    /// 必填：相对项目根的技能目录
    skills_dir: String,
}

/// 内置 agent 名单：`(键, 显示名, 项目相对技能目录)`。
/// 键直接作为 `--agent` 取值；目录值不做命名推断，按名单原样记录。
const BUILTIN: &[(&str, &str, &str)] = &[
    // 通用位：不属于任何具体厂商，所有 agent 共用的目录约定
    ("universal", "通用", ".agents/skills"),
    // 国产 agent：厂商均在中国内地
    ("autoclaw", "AutoClaw", ".openclaw-autoclaw/skills"),
    ("codearts-agent", "CodeArts Agent", ".codeartsdoer/skills"),
    ("codebuddy", "CodeBuddy", ".codebuddy/skills"),
    ("dsh", "DeepSeek Harness", ".dsh/skills"),
    ("iflow-cli", "iFlow CLI", ".iflow/skills"),
    ("kimi-code-cli", "Kimi Code CLI", ".agents/skills"),
    ("lingma", "Lingma", ".lingma/skills"),
    ("minimax-code", "MiniMax Code", ".minimax/skills"),
    ("qoder", "Qoder", ".qoder/skills"),
    ("qoder-cn", "Qoder CN", ".qoder/skills"),
    ("qwen-code", "Qwen Code", ".qwen/skills"),
    ("trae", "Trae", ".trae/skills"),
    ("trae-cn", "Trae CN", ".trae/skills"),
    ("workbuddy", "WorkBuddy", ".workbuddy/skills"),
    ("workbuddy-overseas", "WorkBuddy AI", ".workbuddy-ai/skills"),
    ("zcode", "ZCode", ".zcode/skills"),
    // 国外 agent：其余厂商
    ("adal", "AdaL", ".adal/skills"),
    ("aider-desk", "AiderDesk", ".aider-desk/skills"),
    ("amp", "Amp", ".agents/skills"),
    ("antigravity", "Antigravity", ".agents/skills"),
    ("antigravity-cli", "Antigravity CLI", ".agents/skills"),
    ("astrbot", "AstrBot", "data/skills"),
    ("augment", "Augment", ".augment/skills"),
    ("autohand-code", "Autohand Code CLI", ".autohand/skills"),
    ("bob", "IBM Bob", ".bob/skills"),
    ("claude-code", "Claude Code", ".claude/skills"),
    ("cline", "Cline", ".agents/skills"),
    ("codex", "Codex", ".agents/skills"),
    ("codemaker", "Codemaker", ".codemaker/skills"),
    ("codestudio", "Code Studio", ".codestudio/skills"),
    ("command-code", "Command Code", ".commandcode/skills"),
    ("continue", "Continue", ".continue/skills"),
    ("cortex", "Cortex Code", ".cortex/skills"),
    ("crush", "Crush", ".crush/skills"),
    ("cursor", "Cursor", ".agents/skills"),
    ("deepagents", "Deep Agents", ".agents/skills"),
    ("devin", "Devin for Terminal", ".devin/skills"),
    ("dexto", "Dexto", ".agents/skills"),
    ("droid", "Droid", ".agents/skills"),
    ("eve", "Eve", "agent/skills"),
    ("firebender", "Firebender", ".agents/skills"),
    ("forgecode", "ForgeCode", ".forge/skills"),
    ("fx", "fx", ".agents/skills"),
    ("gemini-cli", "Gemini CLI", ".agents/skills"),
    ("github-copilot", "GitHub Copilot", ".agents/skills"),
    ("goose", "Goose", ".goose/skills"),
    ("grok", "Grok Build", ".grok/skills"),
    ("hermes-agent", "Hermes Agent", ".hermes/skills"),
    ("inference-sh", "inference.sh", ".inferencesh/skills"),
    ("jazz", "Jazz", ".jazz/skills"),
    ("junie", "Junie", ".junie/skills"),
    ("kilo", "Kilo Code", ".agents/skills"),
    ("kimchi", "Kimchi", ".kimchi/skills"),
    ("kiro-cli", "Kiro CLI", ".kiro/skills"),
    ("kode", "Kode", ".kode/skills"),
    ("loaf", "Loaf", ".agents/skills"),
    ("mcpjam", "MCPJam", ".mcpjam/skills"),
    ("mistral-vibe", "Mistral Vibe", ".vibe/skills"),
    ("moxby", "Moxby", ".moxby/skills"),
    ("mux", "Mux", ".mux/skills"),
    ("neovate", "Neovate", ".neovate/skills"),
    ("ona", "Ona", ".ona/skills"),
    ("openclaw", "OpenClaw", "skills"),
    ("opencode", "OpenCode", ".agents/skills"),
    ("openhands", "OpenHands", ".openhands/skills"),
    ("pi", "Pi", ".agents/skills"),
    (
        "posit-assistant",
        "Posit Assistant",
        ".posit/assistant/skills",
    ),
    ("pochi", "Pochi", ".pochi/skills"),
    ("promptscript", "PromptScript", ".agents/skills"),
    ("reasonix", "Reasonix", ".reasonix/skills"),
    ("replit", "Replit", ".agents/skills"),
    ("roo", "Roo Code", ".roo/skills"),
    ("rovodev", "Rovo Dev", ".rovodev/skills"),
    ("sarvam-code", "Sarvam Code", ".agents/skills"),
    ("tabnine-cli", "Tabnine CLI", ".tabnine/agent/skills"),
    ("terramind", "Terramind", ".terramind/skills"),
    ("tinycloud", "Tinycloud", ".tinycloud/skills"),
    ("warp", "Warp", ".agents/skills"),
    ("windsurf", "Windsurf", ".windsurf/skills"),
    ("zed", "Zed", ".agents/skills"),
    ("zencoder", "Zencoder", ".zencoder/skills"),
    ("zenflow", "Zenflow", ".zencoder/skills"),
];

/// 合并内置名单与用户 TOML，只做配置解析，不碰登记簿
pub(crate) fn load_agents(paths: &HubPaths) -> Result<Vec<AgentInfo>, AgentError> {
    let mut merged: BTreeMap<String, AgentInfo> = BUILTIN
        .iter()
        .map(|(key, display_name, skills_dir)| {
            (
                (*key).to_string(),
                AgentInfo {
                    key: (*key).to_string(),
                    display_name: (*display_name).to_string(),
                    skills_dir: (*skills_dir).to_string(),
                    source: AgentSource::BuiltIn,
                    usages: Vec::new(),
                },
            )
        })
        .collect();

    let dir = paths.agents();
    // 耗时起点：扫描是批量文件系统操作，完成时带 elapsed_ms
    let started = Instant::now();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        // 尚未初始化用户覆盖目录 = 只有内置项
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 目录缺失也属扫描正常结束：count 为纯内置名单条目数
            tracing::info!(
                path = %dir.display(),
                count = merged.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "扫描 agent 目录完成"
            );
            return Ok(merged.into_values().collect());
        }
        // 扫描失败在归属它的边界记一次，上层不重复记
        Err(e) => {
            tracing::error!(path = %dir.display(), cause = %e, "扫描 agent 目录失败");
            return Err(e.into());
        }
    };
    for entry in entries {
        let file = entry?.path();
        if !file.is_file() || file.extension().and_then(|e| e.to_str()) != Some("toml") {
            // 跳过非 TOML 条目属可恢复的过滤决策：留 WARN 便于发现放错位置的文件
            tracing::warn!(path = %file.display(), "agent 配置条目不是 TOML，跳过");
            continue;
        }
        let key = file
            .file_stem()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| invalid(&file, "文件名不是合法的 UTF-8 agent 键".into()))?
            .to_string();
        let text = fs::read_to_string(&file)?;
        let cfg: UserAgentConfig =
            toml::from_str(&text).map_err(|e| invalid(&file, format!("TOML 解析失败: {e}")))?;
        validate_skills_dir(&cfg.skills_dir).map_err(|reason| invalid(&file, reason))?;
        let display_name = cfg
            .display_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| key.clone());
        merged.insert(
            key.clone(),
            AgentInfo {
                key,
                display_name,
                skills_dir: cfg.skills_dir,
                source: AgentSource::User,
                usages: Vec::new(),
            },
        );
    }
    // 扫描完成是外部交互里程碑：count 为内置名单与用户覆盖合并后的 agent 总数
    tracing::info!(
        path = %dir.display(),
        count = merged.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "扫描 agent 目录完成"
    );
    Ok(merged.into_values().collect())
}

/// 列出全部可用 agent：配置 + 从登记簿反查的使用记录，按键排序
pub fn list(paths: &HubPaths) -> Result<Vec<AgentInfo>, AgentError> {
    let mut agents = load_agents(paths)?;
    let registry = crate::hub::load_registry(paths)?;
    for agent in &mut agents {
        agent.usages = registry
            .projects
            .iter()
            .flat_map(|record| {
                record
                    .skills
                    .iter()
                    .filter(|skill| skill.agents.contains(&agent.key))
                    .map(move |skill| AgentUsage {
                        project: record.path.clone(),
                        key: skill.key.clone(),
                        version: skill.version.clone(),
                    })
            })
            .collect();
    }
    Ok(agents)
}

/// 按 `--agent` 键解析一个 agent；找不到时把全部认得键带给调用方
pub fn resolve(paths: &HubPaths, key: &str) -> Result<AgentInfo, AgentError> {
    let agents = load_agents(paths)?;
    agents
        .iter()
        .find(|a| a.key == key)
        .cloned()
        .ok_or_else(|| AgentError::UnknownAgent {
            key: key.to_string(),
            known: agents
                .iter()
                .map(|a| a.key.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        })
}

/// `skills_dir` 必须是项目内的相对目录：拒绝空值、绝对路径、盘符与任何
/// 非普通路径段（`.`、`..`、根起点），落点因此不会越出项目根
fn validate_skills_dir(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("skills_dir 不能为空".into());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(format!("skills_dir 必须是相对路径: {raw}"));
    }
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!(
            "skills_dir 不得包含 `.`、`..` 或盘符等非普通路径段: {raw}"
        ));
    }
    Ok(())
}

/// 用户配置错误的统一构造（带文件路径）
fn invalid(path: &Path, reason: String) -> AgentError {
    AgentError::InvalidConfig {
        path: path.display().to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
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
}
