//! 项目面：项目使用（把未归档 skill 按版本铺进项目的 agent 技能目录）与
//! 项目撤回（整体撤回，不提供 agent 粒度）。
//!
//! 首次使用先做全部预检（skill 未归档、无登记、全部落点空闲），任何一项不通过
//! 都不动盘；执行中途失败回滚本次新增落点。撤回以整条技能记录为单位，先检查
//! 全部落点再删除，修改过的 copy 与被替换的落点不会被静默销毁。
//!
//! 本模块为目录模块：使用（use_）、撤回（unuse）、落点解析与落点守卫（dest）
//! 各自独立，此处只留错误类型、报告类型、共用工具与对外重导出。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::agents;
use crate::git;
use crate::hub;
use crate::hub::Scope;
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
    /// 项目路径不存在或不是目录
    #[error("项目目录不存在: {path}")]
    InvalidProject { path: String },
    /// 该项目已有此技能的登记记录
    #[error("该项目已登记此技能，先 unuse 再 use: {key}（项目 {project}）")]
    AlreadyRegistered { key: String, project: String },
    /// 全局作用域已有此技能的登记记录
    #[error("全局已登记此技能，先 unuse 再 use: {key}")]
    AlreadyRegisteredGlobal { key: String },
    /// 一次调用既没给 --global 也没给 --project
    #[error(
        "必须给出至少一个目标：--global 或 --project <目录>（要用当前项目请显式写 --project .）"
    )]
    NoTarget,
    /// 该项目没有此技能的登记记录
    #[error("该项目没有登记此技能，无可撤回: {key}（项目 {project}）")]
    NotRegistered { key: String, project: String },
    /// 落点已被占用，拒绝覆盖
    #[error("落点已存在，拒绝覆盖: {path}")]
    DestinationOccupied { path: String },
    /// 同一次使用的落点产生了不同链接方式，登记簿无法用一个值表达
    #[error("同一次使用的落点链接方式不一致（{kinds:?}），已回滚；请手工处理目录：{paths:?}")]
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
    #[error("copy 落点内容已被修改，拒绝删除: {path}（确认后加 --force）")]
    ModifiedCopy { path: String },
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
            ProjectError::InvalidProject { .. } => "invalid_project",
            ProjectError::AlreadyRegistered { .. } => "already_registered",
            ProjectError::AlreadyRegisteredGlobal { .. } => "already_registered",
            ProjectError::NoTarget => "no_target",
            ProjectError::NotRegistered { .. } => "not_registered",
            ProjectError::DestinationOccupied { .. } => "destination_occupied",
            ProjectError::MixedLinkKinds { .. } => "mixed_link_kinds",
            ProjectError::DirtyExport { .. } => "dirty_export",
            ProjectError::LinkKindMismatch { .. } => "link_kind_mismatch",
            ProjectError::ModifiedCopy { .. } => "modified_copy",
        }
    }
}

/// 一处落点：多个 agent 共用同一实际目录时只出现一次
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    /// 实际落点绝对路径
    pub path: String,
    /// 解析到该落点的 agent 键
    pub agents: Vec<String>,
}

/// 一次调用的目标集合：可选的一个全局目标 + 零到多个项目目标。
/// 集合为空是一次硬错误，由 use 报 `no_target`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UseTargets {
    /// 是否服务全局作用域
    global: bool,
    /// 项目根目录（按调用方给的顺序，可重复）
    projects: Vec<PathBuf>,
}

impl UseTargets {
    /// 从命令行形状构造：`--global` 开关 + 可重复的 `--project`
    pub fn new(global: bool, projects: Vec<PathBuf>) -> Self {
        Self { global, projects }
    }

    /// 只服务全局这一个目标
    pub fn global() -> Self {
        Self::new(true, Vec::new())
    }

    /// 只服务一个项目目标
    pub fn project(project: impl Into<PathBuf>) -> Self {
        Self::new(false, vec![project.into()])
    }

    /// 是否要服务全局目标
    pub(crate) fn wants_global(&self) -> bool {
        self.global
    }

    /// 项目目标目录：按调用方给的顺序，同一目录可能重复出现
    pub(crate) fn project_dirs(&self) -> &[PathBuf] {
        &self.projects
    }
}

/// 一个目标的使用结果：每个目标自带作用域、落点与链接方式
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetOutcome {
    /// 这个目标的作用域
    pub scope: Scope,
    /// 项目根绝对路径；全局目标没有项目，不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// 本次登记的全部 agent（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub agents: Vec<String>,
    /// 实际采用的链接方式（仅实跑出现；作用于该目标全部落点）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkKind>,
    /// 实际建立的落点（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub destinations: Vec<Destination>,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}

/// use 的结果载荷：实跑直出各目标的落点与登记，预演直出各目标的动作清单；
/// 缺席字段经 serde 跳过，预演信封与全部写命令同形
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 固定版本；缺省表示跟随最新（实跑必出现，null 即跟随最新）
    pub version: Option<String>,
    /// 逐个目标的结果，全局目标在最前（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub targets: Vec<TargetOutcome>,
    /// 使用后顺手回收的导出目录统计（实跑必出现，null 即未发生回收）
    pub cleanup: Option<CleanupReport>,
}

/// 使用后顺手回收的统计
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupReport {
    /// 处理的无引用导出目录条目数
    pub removed: usize,
    /// 回收释放的字节数
    pub freed_bytes: u64,
}

/// unuse 的结果载荷：实跑直出撤回报告，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，预演信封与全部写命令同形
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnuseOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 项目根绝对路径（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub project: String,
    /// 实际删除的落点（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub removed: Vec<String>,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
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
mod unuse;
mod use_;

pub(crate) use dest::destination_count;
pub use list::{DestinationView, ProjectSkillView, ProjectView, list};
pub use unuse::unuse;
pub use use_::use_skill;

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
