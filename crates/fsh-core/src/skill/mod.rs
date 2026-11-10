//! 库管理：skill 的入库（add）、发版（release）、归档（archive）、
//! 彻底删除（remove）、恢复（restore）与查询（show / diff）。
//! 版本唯一实体是 git tag；工作副本是编辑面，导出目录由 use 按需生成——
//! 归档只删编辑面，版本链（tag）完好。
//! 写操作的实跑先取进程级文件锁（见 lock 模块）再动盘；预演只读不取锁，
//! 别人正在写时也能给出动作清单。CLI 与 GUI 共用。
//! 本模块为目录模块：add / release / archive / remove / restore / query
//! 各操作子模块共享此处的错误类型、公共类型与内部工具，对外 API 统一经此处重导出发布。
//! 所有收 `<skill>` 的公开入口先过 key 模块的形状判定：键写错报
//! `invalid_skill_key`，不落到「技能不存在」那种误导性报错。

mod add;
mod archive;
pub(crate) mod frontmatter;
mod meta;
mod query;
mod release;
mod remove;
mod restore;
mod state;

// ---------- 公共 API ----------
pub use add::{AddOutcome, add};
pub use archive::{ArchiveOutcome, archive};
pub use query::{diff, show};
pub use release::{ReleaseOutcome, ReleaseSource, release};
pub use remove::{RemoveOutcome, remove};
pub use restore::{RestoreOutcome, restore};
// 技能状态查询供库管理与项目使用两侧共用，不进对外发布面
pub(crate) use state::{SkillState, SkillStatus, StateError, skill_status};

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::git;
use crate::hub;
use crate::lock::{self, WriteToken};
use crate::paths::HubPaths;

use frontmatter::set_metadata_version;

/// 库管理错误
#[derive(Debug, Error)]
pub enum SkillError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// 配置 TOML 解析失败
    #[error("TOML 解析失败: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// 配置 TOML 序列化失败
    #[error("TOML 序列化失败: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// 登记簿 / 配置读写失败
    #[error("hub 配置错误: {0}")]
    Hub(#[from] hub::HubError),
    /// 文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] lock::LockError),
    /// skill 不存在（无工作副本也无版本）。键是 store 内相对路径
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// 同键 skill 已存在（工作副本或版本 tag 占位）
    #[error("skill 已存在: {key}")]
    AlreadyExists { key: String },
    /// skill 处于归档态，该操作只对活跃 skill 开放
    #[error("skill 已归档: {key}")]
    Archived { key: String },
    /// 指定版本不在版本清单里
    #[error("skill 没有该版本: {key}@{version}")]
    VersionNotFound { key: String, version: String },
    /// 目标版本号不大于当前最新版本（号只能前进）
    #[error("目标版本 {target} 不大于当前最新版本 {current}，请改大后再发版: {key}")]
    VersionNotAdvancing {
        key: String,
        current: String,
        target: String,
    },
    /// 版本号不合法（必须三段纯数字 x.y.z）
    #[error("版本号不合法: {version}")]
    InvalidVersion { version: String },
    /// 源目录不满足入库条件
    #[error("无效的 skill 源: {path}（{reason}）")]
    InvalidSource { path: String, reason: String },
    /// 键入参写法非法（形状规则见 key 模块）
    #[error(transparent)]
    InvalidKey(#[from] crate::key::KeyError),
    /// 彻底删除被项目引用拒绝（附引用方项目路径与全局装着它的 agent）
    #[error("skill 仍被引用，先撤回再删: {key}（项目 {projects:?}，全局 agent {global_agents:?}）")]
    Referenced {
        key: String,
        projects: Vec<String>,
        global_agents: Vec<String>,
    },
}

impl ErrorCode for SkillError {
    fn error_code(&self) -> &'static str {
        match self {
            SkillError::Io(_) => "io",
            SkillError::Git(g) => g.error_code(),
            SkillError::TomlDe(_) => "toml_parse",
            SkillError::TomlSer(_) => "toml_serialize",
            SkillError::Hub(h) => h.error_code(),
            SkillError::Lock(l) => l.error_code(),
            SkillError::SkillNotFound { .. } => "skill_not_found",
            SkillError::AlreadyExists { .. } => "already_exists",
            SkillError::Archived { .. } => "archived",
            SkillError::VersionNotFound { .. } => "version_not_found",
            SkillError::VersionNotAdvancing { .. } => "version_not_advancing",
            SkillError::InvalidVersion { .. } => "invalid_version",
            SkillError::InvalidSource { .. } => "invalid_source",
            SkillError::InvalidKey(k) => k.error_code(),
            SkillError::Referenced { .. } => "referenced",
        }
    }
}

// git2 原生错误统一收口到 Git 变体，使 `?` 在全部 git2 调用点直接可用
impl From<git2::Error> for SkillError {
    fn from(e: git2::Error) -> Self {
        SkillError::Git(git::GitError::from(e))
    }
}

// 三态错误由状态模块统一判定并构造，这里收编成库管理侧的变体，
// 文案与稳定码由本枚举持有
impl From<StateError> for SkillError {
    fn from(e: StateError) -> Self {
        match e {
            StateError::SkillNotFound { key } => SkillError::SkillNotFound { key },
            StateError::Archived { key } => SkillError::Archived { key },
            StateError::VersionNotFound { key, version } => {
                SkillError::VersionNotFound { key, version }
            }
        }
    }
}

/// SKILL.md frontmatter：解析 name / description 与 metadata 下的
/// version / source / tags，其余字段不解析不写回（只替换 metadata 里的
/// version 行）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Frontmatter {
    /// skill 名（= SKILL.md 所在目录名 = 唯一键末段）
    pub name: String,
    /// 一句话描述（入库必填）
    pub description: String,
    /// 发版输入：目标版本号（x.y.z），读自 metadata.version；缺省时 add 取 0.1.0
    pub version: Option<String>,
    /// 外部来源（推荐 owner/repo 风格，不校验），读自 metadata.source；自产 skill 可缺省
    pub source: Option<String>,
    /// 标签清单（读自 metadata.tags）
    pub tags: Vec<String>,
}

/// 版本时间线的一项：版本号、发布提交短哈希与提交时间
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VersionEntry {
    /// 版本号（tag 名尾段）
    pub version: String,
    /// 发布提交短哈希（7 位）
    pub commit: String,
    /// 发布提交时间（Unix 秒；轻量 tag 没有 tagger 时间，取提交时间）
    pub date: u64,
}

/// show 的全量详情
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillDetail {
    /// skill 唯一键
    pub key: String,
    /// 最新版本
    pub latest_version: Option<String>,
    /// 是否归档
    pub archived: bool,
    /// 编辑面 frontmatter（活跃态读工作副本，归档态读最新 tag 树；
    /// 读不到或解析不出来时为 None）
    pub frontmatter: Option<Frontmatter>,
    /// frontmatter 解析失败的原因（点名文件与字段；解析成功或读不到文件时为 None）。
    /// 查询面照旧降级不报错，单个技能的详情页据此告诉读的人哪里写错了
    pub frontmatter_error: Option<String>,
    /// 版本时间线（全部版本 tag，新在前）
    pub timeline: Vec<VersionEntry>,
    /// 使用记录：一条登记记录一行，含作用域、项目、agents 与登记版本
    /// （取自 hub 侧登记簿；项目段在前、全局段在后）
    pub usage: Vec<hub::SkillUsage>,
    /// 当前已接入的落点数（登记簿里的项目 × agent 解析去重；
    /// agent 配置读不出来时为 None，表示「未知」而不是「零」）
    pub destination_count: Option<usize>,
    /// SKILL.md 原文（活跃态读工作副本，归档态读最新 tag 树；读不到为 None）
    pub content: Option<String>,
}

/// diff 的单文件结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffFile {
    /// 文件路径（相对 skill 根）
    pub path: String,
    /// unified diff 文本
    pub patch: String,
}

// ---------- 内部工具 ----------

/// 打开 hub 仓库（只读姿势）；装配不完整（缺 .git / .gitignore / hub.toml）
/// 给出结构化错误。判据只查存在，内容完好性归 doctor——判据一旦要读内容
/// 就变得可失败，失败被吞成「未初始化」会让 init 与判据互相矛盾。
/// 预演与查询共用这里：不取写锁，别人正在写时照样能读
fn open_repo(hub_root: &Path) -> Result<(git::Repository, HubPaths), SkillError> {
    let paths = HubPaths::from_root(hub_root);
    let repo = git::open_hub(hub_root)?;
    Ok((repo, paths))
}

/// 取全局写锁并打开仓库（写操作统一入口姿势）
fn lock_and_open(hub_root: &Path) -> Result<(WriteToken, git::Repository, HubPaths), SkillError> {
    let paths = HubPaths::from_root(hub_root);
    let guard = lock::write_lock(&paths)?;
    let (repo, paths) = open_repo(hub_root)?;
    Ok((guard, repo, paths))
}

/// 提交身份：读 hub.toml；未配置时用默认身份
fn identity(paths: &HubPaths) -> Result<hub::UserIdentity, SkillError> {
    Ok(hub::load_config(paths)?.unwrap_or_default().user)
}

/// 无效 frontmatter / 源目录的统一错误构造
pub(super) fn invalid(path: &str, reason: String) -> SkillError {
    SkillError::InvalidSource {
        path: path.to_string(),
        reason,
    }
}

/// 身份约束：frontmatter name 必须等于它所在目录名（键末段）。
/// 入库与发版共用这一套判据，判断与文案只有这一处
fn ensure_name_matches_dir(
    frontmatter: &Frontmatter,
    dir_name: &str,
    label: &str,
) -> Result<(), SkillError> {
    if frontmatter.name == dir_name {
        return Ok(());
    }
    Err(invalid(
        label,
        format!(
            "frontmatter name ({}) 与所在目录名 ({dir_name}) 不一致",
            frontmatter.name
        ),
    ))
}

/// 递归复制目录树，跳过任何深度的 .git 目录（源自身是仓库时不带入嵌套仓库）
fn copy_skill_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_skill_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
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

/// 发版二连：以提交身份收编全部改动 → 打版本 tag。
/// 首版（add）与后续版（release）共用，两条路径的发布语义永远一致。
/// 凭证参数保证两条发布路径都持全局写锁后进入。
fn publish(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
    version: &str,
    message: &str,
    _token: &WriteToken,
) -> Result<(), SkillError> {
    let id = identity(paths)?;
    git::commit_all(repo, message, &id.name, &id.email)?;
    git::tag_version(repo, key, version)?;
    Ok(())
}
