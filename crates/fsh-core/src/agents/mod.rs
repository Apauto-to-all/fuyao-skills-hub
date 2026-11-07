//! agent 配置：内置静态名单 + hub 内用户 TOML 覆盖。
//!
//! agent 只是数据：键是 `--agent` 取值，`skills_dir` 是相对项目根的技能目录，
//! `global_skills_dir` 是可移植写法写的全局技能目录（展开后是绝对路径），
//! 两个目录至少有一个。
//! 内置名单随本程序发布版本更新；用户在 `agents\<键>.toml` 放同名文件即整项
//! 覆盖内置项，放新文件名即新增一个 agent。配置非法一律报错，不回退内置项。

mod builtin;
mod merge;
mod portable_dir;
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
    /// hub 位置解析失败（内置全局目录要用家目录）
    #[error("路径错误: {0}")]
    Paths(#[from] crate::paths::PathsError),
    /// 请求的 agent 没有项目技能目录，项目目标用不了它
    #[error("agent 没有项目技能目录: {key}（该 agent 只能用 --global）")]
    NoProjectDir { key: String },
    /// 请求的 agent 没有全局技能目录，全局目标用不了它
    #[error("agent 没有全局技能目录: {key}（该 agent 只能用 --project）")]
    NoGlobalDir { key: String },
}

impl ErrorCode for AgentError {
    fn error_code(&self) -> &'static str {
        match self {
            AgentError::Io(_) => "io",
            AgentError::InvalidConfig { .. } => "invalid_agent_config",
            AgentError::UnknownAgent { .. } => "unknown_agent",
            AgentError::Registry(error) => error.error_code(),
            AgentError::Paths(error) => error.error_code(),
            AgentError::NoProjectDir { .. } => "agent_no_project_dir",
            AgentError::NoGlobalDir { .. } => "agent_no_global_dir",
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

/// 一个 agent 在某目标里的使用记录（从 hub 侧登记簿反查）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentUsage {
    /// 这条用法属于哪个作用域
    pub scope: crate::hub::Scope,
    /// 项目根绝对路径；全局用法没有项目，不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
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
    /// 相对项目根的技能目录；没有项目位的 agent 不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_dir: Option<String>,
    /// 展开后的全局技能目录绝对路径；没有全局位的 agent 不写
    // TODO: 全局使用与全局撤回（--global）落地前，这个字段只有 `fsh agents` 回显一个读方
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_skills_dir: Option<String>,
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
        let mut usages: Vec<AgentUsage> = registry
            .projects
            .iter()
            .flat_map(|(project, record)| {
                record
                    .skills
                    .iter()
                    .filter(|skill| skill.agents.contains(&agent.key))
                    .map(move |skill| AgentUsage {
                        scope: crate::hub::Scope::Project,
                        project: Some(project.clone()),
                        key: skill.key.clone(),
                        version: skill.version.clone(),
                    })
            })
            .collect();
        usages.extend(
            registry
                .global()
                .skills
                .iter()
                .filter(|skill| skill.agents.contains(&agent.key))
                .map(|skill| AgentUsage {
                    scope: crate::hub::Scope::Global,
                    project: None,
                    key: skill.key.clone(),
                    version: skill.version.clone(),
                }),
        );
        agent.usages = usages;
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
