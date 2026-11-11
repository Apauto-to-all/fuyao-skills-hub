//! 项目面：使用（把 skill 铺进目标里的 agent 技能目录）与撤回（按目标集合
//! 撤回：可只撤指定的 agent，不给名单即整体撤回）。目标可以是全局作用域，
//! 也可以是零到多个项目根。落点一律直连 store 工作副本。
//!
//! 首次使用先做全部预检（skill 在场、无登记、全部落点空闲），任何一项不通过
//! 都不动盘；执行中途失败回滚本次新增落点。撤回先检查全部落点再删除，修改过的
//! copy 与被替换的落点不会被静默销毁；共用落点留到没有 agent 引用它时才删。
//!
//! 本模块为目录模块：使用（use_）、撤回（unuse）、目标集合（target）、
//! 结果载荷（report）、落点解析与落点守卫（dest）、执行骨架共用件（skeleton）
//! 各自独立，此处只留错误类型、共用工具与对外重导出。

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::ErrorCode;
use crate::agents;
use crate::error::{CommonError, common_error_from};
use crate::git;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

/// 项目使用错误
#[derive(Debug, Error)]
pub enum ProjectError {
    /// 底层失败（文件系统、git、文件锁、登记簿、键入参形状）
    #[error(transparent)]
    Common(#[from] CommonError),
    /// agent 配置错误
    #[error("agent 配置错误: {0}")]
    Agent(#[from] agents::AgentError),
    /// 链接引擎错误
    #[error("链接错误: {0}")]
    Link(#[from] link::LinkError),
    /// 键在 hub 里无工作副本
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// skill 已归档：归档态不可用于项目
    #[error("skill 已归档，不能用 use 铺给项目（先 restore）: {key}")]
    Archived { key: String },
    /// 技能说明不是裸键
    #[error("技能说明非法: {spec}（应为裸键 <skill>，不带 @版本）")]
    InvalidSkillSpec { spec: String },
    /// 项目路径不存在或不是目录
    #[error("项目目录不存在: {path}")]
    InvalidProject { path: String },
    /// 该项目已有此技能的登记记录
    #[error("该项目已登记此技能: {key}（项目 {project}）")]
    AlreadyRegistered { key: String, project: String },
    /// 全局作用域已有此技能的登记记录
    #[error("全局已登记此技能: {key}")]
    AlreadyRegisteredGlobal { key: String },
    /// 一次调用既没给 --global 也没给 --project
    #[error(
        "必须给出至少一个目标：--global 或 --project <目录>（要用当前项目请显式写 --project .）"
    )]
    NoTarget,
    /// 该项目没有此技能的登记记录
    #[error("该项目没有登记此技能: {key}（项目 {project}）")]
    NotRegistered { key: String, project: String },
    /// 全局作用域没有此技能的登记记录
    #[error("全局没有登记此技能: {key}")]
    NotRegisteredGlobal { key: String },
    /// 撤回名单里的 agent 不在该记录登记的 agents 里
    #[error(
        "这条记录没有给该 agent 使用，无可撤回: {agent}（技能 {key} 已登记：{known}）；按 agent 撤回只能点名已登记的 agent，不给名单是整体撤回"
    )]
    AgentNotRegistered {
        key: String,
        agent: String,
        known: String,
    },
    /// 落点已被占用，拒绝覆盖
    #[error("落点已存在，拒绝覆盖: {path}")]
    DestinationOccupied { path: String },
    /// 同一条登记的落点产生了不同链接方式，登记簿无法用一个值表达
    #[error("同一条登记的落点链接方式不一致（{kinds:?}），已回滚；请手工处理目录：{paths:?}")]
    MixedLinkKinds {
        kinds: Vec<String>,
        paths: Vec<String>,
    },
    /// 落点实际形态与登记不符
    #[error("落点与登记不符（登记 {expected:?}，实际 {actual}）: {path}")]
    LinkKindMismatch {
        path: String,
        expected: LinkKind,
        actual: String,
    },
    /// copy 落点内容与当前源不一致
    #[error("copy 落点内容已被修改，拒绝删除或换靶: {path}（确认后加 --force）")]
    ModifiedCopy { path: String },
}

// 三态错误由技能状态模块统一判定并构造，这里收编进项目使用的文案与稳定码；
// 项目侧的归档文案带操作指引，与库管理侧的说法各自保留
impl From<crate::skill::StateError> for ProjectError {
    fn from(e: crate::skill::StateError) -> Self {
        match e {
            crate::skill::StateError::SkillNotFound { key } => ProjectError::SkillNotFound { key },
            crate::skill::StateError::Archived { key } => ProjectError::Archived { key },
        }
    }
}

common_error_from!(
    ProjectError: std::io::Error,
    git::GitError,
    hub::HubError,
    crate::lock::LockError,
    crate::key::KeyError
);

impl ErrorCode for ProjectError {
    fn error_code(&self) -> &'static str {
        match self {
            ProjectError::Common(c) => c.error_code(),
            ProjectError::Agent(a) => a.error_code(),
            ProjectError::Link(l) => l.error_code(),
            ProjectError::SkillNotFound { .. } => "skill_not_found",
            ProjectError::Archived { .. } => "archived",
            ProjectError::InvalidSkillSpec { .. } => "invalid_skill_spec",
            ProjectError::InvalidProject { .. } => "invalid_project",
            ProjectError::AlreadyRegistered { .. } => "already_registered",
            ProjectError::AlreadyRegisteredGlobal { .. } => "already_registered",
            ProjectError::NoTarget => "no_target",
            ProjectError::NotRegistered { .. } => "not_registered",
            ProjectError::NotRegisteredGlobal { .. } => "not_registered",
            ProjectError::AgentNotRegistered { .. } => "agent_not_registered",
            ProjectError::DestinationOccupied { .. } => "destination_occupied",
            ProjectError::MixedLinkKinds { .. } => "mixed_link_kinds",
            ProjectError::LinkKindMismatch { .. } => "link_kind_mismatch",
            ProjectError::ModifiedCopy { .. } => "modified_copy",
        }
    }
}

/// 解析 `<skill>`：只收裸键，带 `@` 一律按非法技能说明拒绝
pub fn split_skill_spec(spec: &str) -> Result<String, ProjectError> {
    if spec.contains('@') || spec.is_empty() {
        return Err(ProjectError::InvalidSkillSpec {
            spec: spec.to_string(),
        });
    }
    // 键先过形状判定：写错的键当场点名，别落到「技能不存在」
    crate::key::validate(spec)?;
    Ok(spec.to_string())
}

mod dest;
mod list;
mod report;
mod skeleton;
mod target;
mod unuse;
mod use_;

/// 只读解析登记簿里全部记录的落点路径（doctor 孤儿落点核对用）。
/// agent 配置读不出时返回空集，由调用方按「无法核对」处理
pub(crate) fn registered_destinations(paths: &HubPaths) -> Vec<String> {
    let Ok(registry) = hub::load_registry(paths) else {
        return Vec::new();
    };
    let Ok(known) = agents::list(paths) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (path, record) in &registry.projects {
        for skill in &record.skills {
            if let Ok(dests) =
                dest::resolve_project_pub(&known, Path::new(path), &skill.agents, &skill.key)
            {
                out.extend(dests.into_iter().map(|d| d.path));
            }
        }
    }
    for skill in &registry.global().skills {
        if let Ok(dests) = dest::resolve_global_pub(&known, &skill.agents, &skill.key) {
            out.extend(dests.into_iter().map(|d| d.path));
        }
    }
    out
}

pub use list::{DestinationView, ProjectSkillView, ProjectView, list};
pub use report::{Destination, TargetOutcome, UnuseOutcome, UnuseTarget, UseOutcome};
pub(crate) use target::Anchor;
pub use target::Targets;
pub use unuse::unuse;
pub use use_::use_skill;

/// 取某目标里此技能的登记记录：撤回要先按目标找到那条记录，
/// 没有记录即无可操作
pub(super) fn registered_skill<'a>(
    registry: &'a hub::ProjectRegistry,
    anchor: &Anchor,
    key: &str,
) -> Result<&'a hub::ProjectSkill, ProjectError> {
    let found = match anchor {
        Anchor::Project(dir) => registry
            .project(dir)
            .and_then(|record| record.skills.iter().find(|s| s.key == key)),
        Anchor::Global => registry.global().skills.iter().find(|s| s.key == key),
    };
    found.ok_or_else(|| match anchor {
        Anchor::Project(dir) => ProjectError::NotRegistered {
            key: key.into(),
            project: crate::identity::display_path(dir),
        },
        Anchor::Global => ProjectError::NotRegisteredGlobal { key: key.into() },
    })
}

/// 倒序撤销已建落点：失败也继续删其余入口，保证不留下半套入口
fn rollback_links(created: &[(PathBuf, LinkKind)]) {
    for (path, kind) in created.iter().rev() {
        // 回滚删不掉会留下半套入口，是唯一能发现残留的地方，必须留痕
        if let Err(e) = link::remove_link(path, *kind) {
            tracing::warn!(path = %path.display(), cause = %e, "回滚落点删除失败");
        }
    }
}

/// 项目根规范化：必须是已存在的目录，调用方据此拿到唯一的绝对身份
fn canonical_project(project_dir: &Path) -> Result<PathBuf, ProjectError> {
    let canonical = fs::canonicalize(project_dir).map_err(|_| ProjectError::InvalidProject {
        path: project_dir.display().to_string(),
    })?;
    if !canonical.is_dir() {
        return Err(ProjectError::InvalidProject {
            path: project_dir.display().to_string(),
        });
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests;
