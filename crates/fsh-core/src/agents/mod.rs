//! agent 配置：内置静态名单 + hub 内用户 TOML 覆盖。
//!
//! agent 只是数据：键是 `--agent` 取值，`skills_dir` 是相对项目根的技能目录，
//! `global_skills_dir` 是可移植写法写的全局技能目录，两个目录至少有一个。
//! 读模型只带配置本身；全局落点在用时由 `expand_global` 从配置原文展开。
//! 内置名单随本程序发布版本更新；用户在 `agents\<键>.toml` 放同名文件即整项
//! 覆盖内置项，放新文件名即新增一个 agent。配置非法一律报错，不回退内置项。

mod builtin;
mod global_root;
mod merge;
mod portable_dir;
#[cfg(test)]
mod tests;

pub use merge::list;

pub(crate) use global_root::GlobalRoot;

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

use crate::ErrorCode;
use crate::error::{CommonError, common_error_from};
use crate::paths::PathsError;

/// agent 配置错误
#[derive(Debug, Error)]
pub enum AgentError {
    /// 底层失败（文件系统）
    #[error(transparent)]
    Common(#[from] CommonError),
    /// 用户 TOML 无法解析或字段取值非法
    #[error("agent 配置非法: {path}（{reason}）")]
    InvalidConfig { path: String, reason: String },
    /// 请求的 agent 不在内置与用户配置的并集里
    #[error("不认识的 agent: {key}（认得的 agent: {known}）")]
    UnknownAgent { key: String, known: String },
    /// hub 位置解析失败（全局目录展开要用家目录）
    #[error("路径错误: {0}")]
    Paths(#[from] crate::paths::PathsError),
    /// 请求的 agent 没有项目技能目录，项目目标用不了它
    #[error("agent 没有项目技能目录: {key}（该 agent 只能用 --global）")]
    NoProjectDir { key: String },
    /// 请求的 agent 没有全局技能目录，全局目标用不了它
    #[error("agent 没有全局技能目录: {key}（该 agent 只能用 --project）")]
    NoGlobalDir { key: String },
}

common_error_from!(AgentError: std::io::Error);

impl ErrorCode for AgentError {
    fn error_code(&self) -> &'static str {
        match self {
            AgentError::Common(c) => c.error_code(),
            AgentError::InvalidConfig { .. } => "invalid_agent_config",
            AgentError::UnknownAgent { .. } => "unknown_agent",
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

/// 一个可用 agent 的公共视图：只有配置本身
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentInfo {
    /// agent 键（`--agent` 取值，等于 TOML 文件名去扩展名）
    pub key: String,
    /// 给人看的名字
    pub display_name: String,
    /// 相对项目根的技能目录；没有项目位的 agent 不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_dir: Option<String>,
    /// 配置里写的全局技能目录原文（可移植写法，如 `~/.dsh/skills`）；没有全局位的 agent 不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_skills_dir: Option<String>,
    /// 内置条目的全局根适配声明；用户 TOML 配置一律为 None，
    /// 序列化不携带——列表输出只回显配置原文
    #[serde(skip)]
    pub(crate) global_root: Option<GlobalRoot>,
    /// 配置来源
    pub source: AgentSource,
}

/// 把配置里的全局技能目录写法展开成绝对路径：项目落点用相对目录现算，全局落点靠它。
/// `root` 是内置条目携带的全局根适配声明：解析时读变量、探测存在性，变量未设置
/// 或取值不可用时静默回落字面量；用户 TOML 条目传 None，显式写未设置的变量仍报错。
/// `at` 是报错时点名的位置（配置文件路径或 agent 键）
pub(crate) fn expand_global(
    raw: &str,
    root: Option<GlobalRoot>,
    at: &str,
) -> Result<PathBuf, AgentError> {
    let home = std::env::home_dir().ok_or(PathsError::NoHome)?;
    let lookup = |name: &str| std::env::var(name).ok();
    let map_err = |reason: String| AgentError::InvalidConfig {
        path: at.to_string(),
        reason: format!("global_skills_dir {reason}"),
    };
    match root {
        Some(root) => {
            global_root::resolve(root, raw, &home, cfg!(windows), &lookup).map_err(map_err)
        }
        None => portable_dir::expand(raw, &home, &lookup).map_err(map_err),
    }
}
