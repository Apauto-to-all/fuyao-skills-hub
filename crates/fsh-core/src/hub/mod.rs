//! hub 装配面：一条龙初始化、hub.toml 配置读写与项目登记簿。
//! 本模块为目录模块：装配（init）、配置（config）、登记簿（registry）
//! 各自独立，此处只留错误类型、共用小件与对外重导出；对外入口只有
//! `init`，装配细节收在模块内部。

use crate::ErrorCode;
use crate::git;
use thiserror::Error;

/// hub 装配错误
#[derive(Debug, Error)]
pub enum HubError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// hub.toml 解析失败
    #[error("hub.toml 解析失败: {0}")]
    ConfigTomlDe(toml::de::Error),
    /// hub.toml 序列化失败
    #[error("hub.toml 序列化失败: {0}")]
    ConfigTomlSer(toml::ser::Error),
    /// projects.toml 解析失败
    #[error("projects.toml 解析失败: {0}")]
    RegistryTomlDe(toml::de::Error),
    /// projects.toml 序列化失败
    #[error("projects.toml 序列化失败: {0}")]
    RegistryTomlSer(toml::ser::Error),
    /// 进程级文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] crate::lock::LockError),
}

impl ErrorCode for HubError {
    fn error_code(&self) -> &'static str {
        match self {
            HubError::Io(_) => "io",
            HubError::Git(g) => g.error_code(),
            HubError::ConfigTomlDe(_) | HubError::RegistryTomlDe(_) => "toml_parse",
            HubError::ConfigTomlSer(_) | HubError::RegistryTomlSer(_) => "toml_serialize",
            HubError::Lock(l) => l.error_code(),
        }
    }
}

/// hub 仓库的忽略规则：白名单——先忽略根下一切，再放行属于仓库的固定项与全部 tracked 面。
/// 运行期日志、项目登记簿（含本机绝对路径）、写锁文件，以及将来新增的
/// 任何根级产物都天然被 `/*` 挡住，不必逐条追加。
/// 放行哪些面由布局模块的 [`crate::paths::FACES`] 给出
pub fn gitignore_content() -> String {
    let mut text = String::from("/*\n!/.gitignore\n!/hub.toml\n");
    for face in crate::paths::FACES {
        text.push_str(&format!("!/{face}/\n"));
    }
    text
}

/// .gitignore 是否就是白名单原文：派生物，内容不符即视为漂移
pub fn gitignore_is_canonical(paths: &crate::paths::HubPaths) -> bool {
    std::fs::read_to_string(paths.gitignore())
        .map(|text| text == gitignore_content())
        .unwrap_or(false)
}

/// 当前 Unix 秒（登记时间戳的统一来源）
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

mod config;
mod init;
mod materialize;
mod registry;

pub use config::{HubConfig, UpdateConfig, UpdateMode, load_config};
pub(crate) use config::{log_config_error, read_config};
pub use init::{InitOutcome, init};
pub(crate) use materialize::{orphan_names, workdir_matches};
pub use registry::{
    GlobalSkills, ProjectRecord, ProjectRegistry, ProjectSkill, RecordRef, Scope, SkillUsage,
    load_registry, save_registry,
};

#[cfg(test)]
mod tests;
