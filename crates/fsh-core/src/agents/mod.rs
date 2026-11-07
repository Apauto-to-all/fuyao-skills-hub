//! agent 配置：内置静态名单 + hub 内用户 TOML 覆盖。
//!
//! agent 只是数据：键是 `--agent` 取值，`skills_dir` 是相对项目根的技能目录。
//! 内置名单随本程序发布版本更新；用户在 `agents\<键>.toml` 放同名文件即整项
//! 覆盖内置项，放新文件名即新增一个 agent。配置非法一律报错，不回退内置项。

mod builtin;
mod merge;
#[cfg(test)]
mod tests;

pub(crate) use merge::load_agents;

use serde::Serialize;
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

/// 列出全部可用 agent：配置 + 从登记簿反查的使用记录，按键排序
pub fn list(paths: &HubPaths) -> Result<Vec<AgentInfo>, AgentError> {
    let mut agents = load_agents(paths)?;
    let registry = crate::hub::load_registry(paths)?;
    for agent in &mut agents {
        agent.usages = registry
            .projects
            .iter()
            .flat_map(|(project, record)| {
                record
                    .skills
                    .iter()
                    .filter(|skill| skill.agents.contains(&agent.key))
                    .map(move |skill| AgentUsage {
                        project: project.clone(),
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
