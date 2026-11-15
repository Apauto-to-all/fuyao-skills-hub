//! 库管理：skill 的入库（add）、归档（archive）、恢复（restore）、
//! 彻底删除（remove）、列表（list）。工作副本是唯一编辑面；hub 的 git
//! 是自动审计日志，写命令落盘成功后各产生一条机器提交（见 git::auto_commit）。
//! 写操作的实跑先取进程级文件锁（见 lock 模块）再动盘；预演只读不取锁，
//! 别人正在写时也能给出动作清单。CLI 与 GUI 共用。
//! 本模块为目录模块：add / archive / restore / remove / list
//! 各操作子模块共享此处的错误类型与公共类型，对外 API 统一经此处重导出发布。
//! 所有收 `<skill>` 的公开入口先过 key 模块的形状判定：键写错报
//! `invalid_skill_key`，不落到「技能不存在」那种误导性报错。

mod add;
mod archive;
pub(crate) mod frontmatter;
mod list;
mod move_;
mod outcome;
mod remove;
mod restore;
mod state;

// ---------- 公共 API ----------
pub use add::add;
pub use archive::archive;
pub use list::{SkillRow, list};
pub use move_::{MoveOutcome, move_skill};
pub use outcome::SkillOutcome;
pub use remove::remove;
pub use restore::restore;
// 技能占位、落点占用与技能识别供库管理与项目使用两侧共用，不进对外发布面
pub(crate) use state::{
    ActiveError, FoundSkill, Occupancy, RestoreEnds, StateError, occupancy, skill_occupancy,
    walk_face,
};

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::error::{CommonError, common_error_from};
use crate::git;
use crate::hub;
use crate::lock;
use crate::paths::HubPaths;

/// 库管理错误
#[derive(Debug, Error)]
pub enum SkillError {
    /// 底层失败（文件系统、git、登记簿、文件锁、键入参形状）
    #[error(transparent)]
    Common(#[from] CommonError),
    /// skill 不存在（store 与 archive 下皆无）。键是 store 内相对路径
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// 同键 skill 已存在（工作副本或归档副本占位）
    #[error("skill 已存在: {key}")]
    AlreadyExists { key: String },
    /// skill 已归档，该操作只对活跃 skill 开放
    #[error("skill 已归档: {key}")]
    Archived { key: String },
    /// skill 已归档，键迁移只对活跃面开放（归档面不提供重组）
    #[error("skill 已归档，不能 move（先 restore 回 store 再重组）: {key}")]
    ArchivedMove { key: String },
    /// 目标键已占用（归档 / 恢复的目标面已有同名键）
    #[error("目标键已占用: {key}")]
    Conflict { key: String },
    /// 源目录不满足入库条件
    #[error("无效的 skill 源: {path}（{reason}）")]
    InvalidSource { path: String, reason: String },
    /// 彻底删除被项目引用拒绝（附引用方项目路径与全局装着它的 agent）
    #[error("skill 仍被引用，先撤回再删: {key}（项目 {projects:?}，全局 agent {global_agents:?}）")]
    Referenced {
        key: String,
        projects: Vec<String>,
        global_agents: Vec<String>,
    },
    /// move 要求新旧键末段一致：末段是身份，改身份走整目录改名后重新 add
    #[error("键末段是技能身份，不可经 move 变更: {from} -> {to}（改身份走整目录改名 + 重新 add）")]
    IdentityImmutable { from: String, to: String },
    /// 键迁移的项目面失败（agent 配置、落点形态与链接）：文案与稳定码照项目面原样
    #[error(transparent)]
    Migrate(#[from] crate::project::ProjectError),
}

common_error_from!(
    SkillError: std::io::Error,
    git::GitError,
    hub::HubError,
    lock::LockError,
    crate::key::KeyError
);

impl ErrorCode for SkillError {
    fn error_code(&self) -> &'static str {
        match self {
            SkillError::Common(c) => c.error_code(),
            SkillError::SkillNotFound { .. } => "skill_not_found",
            SkillError::AlreadyExists { .. } => "already_exists",
            SkillError::Archived { .. } => "archived",
            SkillError::ArchivedMove { .. } => "archived",
            SkillError::Conflict { .. } => "conflict",
            SkillError::InvalidSource { .. } => "invalid_source",
            SkillError::Referenced { .. } => "referenced",
            SkillError::IdentityImmutable { .. } => "identity_immutable",
            SkillError::Migrate(p) => p.error_code(),
        }
    }
}

// git2 原生错误统一收口到公共底层错误的 Git 变体，
// 使 `?` 在全部 git2 调用点直接可用
impl From<git2::Error> for SkillError {
    fn from(e: git2::Error) -> Self {
        git::GitError::from(e).into()
    }
}

// 占位与前置拒绝由状态模块统一判定并构造，这里收编成库管理侧的变体，
// 文案与稳定码由本枚举持有
impl From<StateError> for SkillError {
    fn from(e: StateError) -> Self {
        match e {
            StateError::SkillNotFound { key } => SkillError::SkillNotFound { key },
            StateError::Archived { key } => SkillError::Archived { key },
            StateError::Conflict { key } => SkillError::Conflict { key },
            StateError::AlreadyExists { key } => SkillError::AlreadyExists { key },
        }
    }
}

// 「要求活跃」的两种拒绝同样收编成库管理侧的变体
impl From<ActiveError> for SkillError {
    fn from(e: ActiveError) -> Self {
        match e {
            ActiveError::SkillNotFound { key } => SkillError::SkillNotFound { key },
            ActiveError::Archived { key } => SkillError::Archived { key },
        }
    }
}

/// SKILL.md frontmatter：解析 name / description
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Frontmatter {
    /// skill 名（= SKILL.md 所在目录名 = 唯一键末段）
    pub name: String,
    /// 一句话描述（入库必填）
    pub description: String,
}

// ---------- 内部工具 ----------

/// 无效 frontmatter / 源目录的统一错误构造
pub(super) fn invalid(path: &str, reason: String) -> SkillError {
    SkillError::InvalidSource {
        path: path.to_string(),
        reason,
    }
}

/// 引用判定：项目与全局两段一起看，只要登记簿里还有这条技能就算被引用。
/// 报错按作用域分列——项目路径各占一项，全局段归并成一份去重排序的 agent 名单
fn ensure_unreferenced(paths: &HubPaths, key: &str) -> Result<(), SkillError> {
    let registry = hub::load_registry(paths)?;
    let usages = registry.usages(key);
    if usages.is_empty() {
        return Ok(());
    }
    let mut projects: Vec<String> = Vec::new();
    let mut global_agents: Vec<String> = Vec::new();
    for usage in usages {
        match usage.scope {
            hub::Scope::Project => {
                if let Some(project) = usage.project {
                    projects.push(project);
                }
            }
            hub::Scope::Global => global_agents.extend(usage.agents),
        }
    }
    global_agents.sort();
    global_agents.dedup();
    Err(SkillError::Referenced {
        key: key.into(),
        projects,
        global_agents,
    })
}
