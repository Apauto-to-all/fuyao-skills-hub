//! 部署引擎：plan（纯计算变更清单）→ apply（物化 + 链接 + 记账 + 顺手清）
//! → status（双侧漂移检测）。项目侧三件套：skills.toml（人写声明）、
//! skills.lock.toml（机器账本，可提交）、state.toml（本机链接状态，不提交）。
//! 版本引用只支持精确 semver 与 stable（stable = 最新版本 tag）。
//! 本模块为目录模块：model（三件套数据模型与内容哈希）、plan、apply、
//! state（漂移检查）四个子模块共享此处的错误类型与仓库入口，对外 API 统一经此处重导出发布。

mod apply;
mod model;
mod plan;
mod state;

// ---------- 公共 API ----------
pub use apply::apply;
pub use model::{LockEntry, LockFile, ProjectState, SkillReq, SkillsManifest, StateLink};
pub use plan::{Change, plan};
pub use state::{Drift, status};

#[cfg(test)]
mod tests;

// gc / doctor / skill 跨模块取用的内部读口与哈希口径
pub(crate) use model::{hash_dir_contents, hash_tag_contents, read_lock};

use std::path::Path;

use thiserror::Error;

use crate::ErrorCode;
use crate::git;
use crate::link;

/// 部署引擎错误
#[derive(Debug, Error)]
pub enum DeployError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// TOML 解析失败（skills.toml / skills.lock.toml / state.toml）
    #[error("TOML 解析失败: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// TOML 序列化失败（skills.lock.toml / state.toml）
    #[error("TOML 序列化失败: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// hub.toml 缺失或损坏（hub 未初始化）
    #[error("hub 未初始化: {0}")]
    HubNotInitialized(String),
    /// 声明里引用的版本在 hub 中不存在
    #[error("版本不存在: {skill}@{version}")]
    VersionNotFound { skill: String, version: String },
    /// 链接操作失败
    #[error("链接错误: {0}")]
    Link(#[from] link::LinkError),
    /// 登记簿读写失败（apply 收尾登记）
    #[error("登记簿错误: {0}")]
    Hub(#[from] crate::hub::HubError),
    /// 进程级文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] crate::lock::LockError),
}

impl ErrorCode for DeployError {
    fn error_code(&self) -> &'static str {
        match self {
            DeployError::Io(_) => "io",
            DeployError::Git(g) => g.error_code(),
            DeployError::TomlDe(_) => "toml_parse",
            DeployError::TomlSer(_) => "toml_serialize",
            DeployError::HubNotInitialized(_) => "hub_not_initialized",
            DeployError::VersionNotFound { .. } => "version_not_found",
            DeployError::Link(l) => l.error_code(),
            DeployError::Hub(h) => h.error_code(),
            DeployError::Lock(l) => l.error_code(),
        }
    }
}

// git2 原生错误统一收口到 Git 变体，使 `?` 在全部 git2 调用点直接可用
impl From<git2::Error> for DeployError {
    fn from(e: git2::Error) -> Self {
        DeployError::Git(git::GitError::from(e))
    }
}

/// 打开 hub 仓库；未初始化时给出结构化错误
fn open_repo(hub_root: &Path) -> Result<git2::Repository, DeployError> {
    if !hub_root.join(".git").exists() {
        return Err(DeployError::HubNotInitialized(
            hub_root.display().to_string(),
        ));
    }
    Ok(git2::Repository::open(hub_root).map_err(git::GitError::from)?)
}
