//! 项目面：使用（把未归档 skill 按版本铺进目标里的 agent 技能目录）与
//! 撤回（按目标集合撤回：可只撤指定的 agent，不给名单即整体撤回）。目标可以是
//! 全局作用域，也可以是零到多个项目根。
//!
//! 首次使用先做全部预检（skill 未归档、无登记、全部落点空闲），任何一项不通过
//! 都不动盘；执行中途失败回滚本次新增落点。撤回先检查全部落点再删除，修改过的
//! copy 与被替换的落点不会被静默销毁；共用落点留到没有 agent 引用它时才删。
//!
//! 版本切换（upgrade）只改已有记录：旧入口改名占位 → 原路径建新入口 →
//! 全部成功才删占位，任一失败整批还原且登记簿不动；换之前逐落点过三闸——
//! 形态必须与登记档位一致、链接靶子必须仍是记录推导出的旧源、拷贝内容不得被手改
//! （`--force` 只覆盖拷贝这一项）。已是目标版本时零盘动作。
//!
//! 本模块为目录模块：使用（use_）、撤回（unuse）、版本切换（upgrade）、目标集合
//! （target）、结果载荷（report）、落点解析与落点守卫（dest）各自独立，
//! 此处只留错误类型、共用工具与对外重导出。

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::ErrorCode;
use crate::agents;
use crate::git;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::lock::WriteToken;
use crate::paths::HubPaths;

/// 项目使用错误
#[derive(Debug, Error)]
pub enum ProjectError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// 登记簿读写失败
    #[error("登记簿错误: {0}")]
    Registry(#[from] hub::HubError),
    /// 文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] crate::lock::LockError),
    /// agent 配置错误
    #[error("agent 配置错误: {0}")]
    Agent(#[from] agents::AgentError),
    /// 链接引擎错误
    #[error("链接错误: {0}")]
    Link(#[from] link::LinkError),
    /// hub 装配不完整（缺 .git / .gitignore / hub.toml）
    #[error("hub 未初始化或装配不完整: {0}")]
    HubNotInitialized(String),
    /// 键在 hub 里既无工作副本也无版本 tag
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// skill 已归档：归档态不可用于项目
    #[error("skill 已归档，不能用 use 铺给项目（先 restore）: {key}")]
    Archived { key: String },
    /// 指定版本不存在
    #[error("skill 没有该版本: {key}@{version}")]
    VersionNotFound { key: String, version: String },
    /// 技能说明不符合 `<skill>[@<version>]` 形状
    #[error("技能说明非法: {spec}（应为 <skill> 或 <skill>@<x.y.z>）")]
    InvalidSkillSpec { spec: String },
    /// 键入参写法非法（形状规则见 key 模块）
    #[error(transparent)]
    InvalidKey(#[from] crate::key::KeyError),
    /// 项目路径不存在或不是目录
    #[error("项目目录不存在: {path}")]
    InvalidProject { path: String },
    /// 该项目已有此技能的登记记录
    #[error("该项目已登记此技能，版本切换改用 fsh upgrade {key}[@<版本>]（项目 {project}）")]
    AlreadyRegistered { key: String, project: String },
    /// 全局作用域已有此技能的登记记录
    #[error("全局已登记此技能，版本切换改用 fsh upgrade {key}[@<版本>]")]
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
    /// 导出目录内容与 tag 不符
    #[error("导出目录内容与 tag 不符，保留现场不覆盖: {path}")]
    DirtyExport { path: String },
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
    /// 版本切换会在落点旁留下的占位名已被占用：上次中断的残留，须人工删除
    #[error("占位目录残留（需人工删除后再做版本切换）: {path}")]
    PlaceholderResidue { path: String },
    /// 记录里某 agent 的落点已不存在
    #[error(
        "落点已不存在，版本切换不动盘: {path}（目标 {target}，agents: {agents}）；先按 agent 撤回（fsh unuse {key} --agent <名单>），或确认要重建就直接重新 fsh use {key}"
    )]
    DestinationMissing {
        key: String,
        target: String,
        path: String,
        agents: String,
    },
}

impl ErrorCode for ProjectError {
    fn error_code(&self) -> &'static str {
        match self {
            ProjectError::Io(_) => "io",
            ProjectError::Git(g) => g.error_code(),
            ProjectError::Registry(h) => h.error_code(),
            ProjectError::Lock(l) => l.error_code(),
            ProjectError::Agent(a) => a.error_code(),
            ProjectError::Link(l) => l.error_code(),
            ProjectError::HubNotInitialized(_) => "hub_not_initialized",
            ProjectError::SkillNotFound { .. } => "skill_not_found",
            ProjectError::Archived { .. } => "archived",
            ProjectError::VersionNotFound { .. } => "version_not_found",
            ProjectError::InvalidSkillSpec { .. } => "invalid_skill_spec",
            ProjectError::InvalidKey(k) => k.error_code(),
            ProjectError::InvalidProject { .. } => "invalid_project",
            ProjectError::AlreadyRegistered { .. } => "already_registered",
            ProjectError::AlreadyRegisteredGlobal { .. } => "already_registered",
            ProjectError::NoTarget => "no_target",
            ProjectError::NotRegistered { .. } => "not_registered",
            ProjectError::NotRegisteredGlobal { .. } => "not_registered",
            ProjectError::AgentNotRegistered { .. } => "agent_not_registered",
            ProjectError::DestinationOccupied { .. } => "destination_occupied",
            ProjectError::MixedLinkKinds { .. } => "mixed_link_kinds",
            ProjectError::DirtyExport { .. } => "dirty_export",
            ProjectError::LinkKindMismatch { .. } => "link_kind_mismatch",
            ProjectError::ModifiedCopy { .. } => "modified_copy",
            ProjectError::PlaceholderResidue { .. } => "placeholder_residue",
            ProjectError::DestinationMissing { .. } => "destination_missing",
        }
    }
}

/// 解析 `<skill>[@<版本>]`：版本须为三段数字，缺省表示跟随最新
pub fn split_skill_spec(spec: &str) -> Result<(String, Option<String>), ProjectError> {
    let (key, version) = match spec.rsplit_once('@') {
        Some((key, version)) => (key, Some(version)),
        None => (spec, None),
    };
    if key.is_empty() {
        return Err(ProjectError::InvalidSkillSpec {
            spec: spec.to_string(),
        });
    }
    // 键先过形状判定：`\` 这类写错要当场点名，别落到「技能不存在」
    crate::key::validate(key)?;
    if let Some(v) = version
        && crate::version::Version::parse(v).is_none()
    {
        return Err(ProjectError::InvalidSkillSpec {
            spec: spec.to_string(),
        });
    }
    Ok((key.to_string(), version.map(str::to_string)))
}

mod dest;
mod list;
mod report;
mod target;
mod unuse;
mod upgrade;
mod use_;

pub(crate) use dest::destination_count;
pub use list::{DestinationView, ProjectSkillView, ProjectView, list};
pub use report::{
    CleanupReport, Destination, TargetOutcome, UnuseOutcome, UnuseTarget, UpgradeOutcome,
    UpgradeTarget, UseOutcome,
};
pub use target::Targets;
pub(crate) use target::{Anchor, anchors_of};
pub use unuse::unuse;
pub use upgrade::upgrade;
pub use use_::use_skill;

/// 取某目标里此技能的登记记录：版本切换与撤回都要先按目标找到那条记录，
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

/// 打开 hub 仓库（只读姿势）：装配不完整给出结构化错误。
/// 预演与查询共用这里：不取写锁，别人正在写时照样能读
fn open_repo(hub_root: &Path) -> Result<(git::Repository, HubPaths), ProjectError> {
    let paths = HubPaths::from_root(hub_root);
    let repo = git::open_hub(hub_root)?;
    Ok((repo, paths))
}

/// 建链接 / 删链接前的统一入口：确认 hub 已初始化并取全局写锁
fn lock_and_open(hub_root: &Path) -> Result<(WriteToken, git::Repository, HubPaths), ProjectError> {
    let paths = HubPaths::from_root(hub_root);
    if !paths.is_initialized() {
        return Err(ProjectError::HubNotInitialized(
            hub_root.display().to_string(),
        ));
    }
    let token = crate::lock::write_lock(&paths)?;
    let repo = git::open_hub(hub_root)?;
    Ok((token, repo, paths))
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
