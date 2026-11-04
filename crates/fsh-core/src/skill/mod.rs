//! 库管理：skill 的入库（add）、发版（release）、归档（archive）、
//! 彻底删除（remove）、恢复（restore）与查询（list / show / diff）。
//! 版本唯一实体是 git tag；工作副本是编辑面，导出目录由 use 按需生成——
//! 归档只删编辑面，版本链（tag）完好。
//! 全部写操作先取进程级文件锁（见 lock 模块），CLI 与 GUI 共用。
//! 本模块为目录模块：add / release / archive / remove / restore / query
//! 各操作子模块共享此处的错误类型、公共类型与内部工具，对外 API 统一经此处重导出发布。

mod add;
mod archive;
mod frontmatter;
mod query;
mod release;
mod remove;
mod restore;

// ---------- 公共 API ----------
pub use add::{add, add_dry};
pub use archive::{archive, archive_dry};
pub use query::{diff, list, show};
pub use release::{release, release_dry};
pub use remove::{remove, remove_dry};
pub use restore::{restore, restore_dry};

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

use frontmatter::{cmp_semver, parse, set_frontmatter_version};

/// 三段纯数字版本号校验的对外出口
pub(crate) use frontmatter::valid_semver;

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
    /// hub 未初始化（无 .git）
    #[error("hub 未初始化: {0}")]
    HubNotInitialized(String),
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
    /// 目标版本号不大于当前最新版本（号只能前进）
    #[error("目标版本 {target} 不大于当前最新版本 {current}，请改大后再发版: {key}")]
    VersionNotAdvancing {
        key: String,
        current: String,
        target: String,
    },
    /// 源目录不满足入库条件
    #[error("无效的 skill 源: {path}（{reason}）")]
    InvalidSource { path: String, reason: String },
    /// 彻底删除被项目引用拒绝（附引用方项目路径）
    #[error("skill 仍被项目引用，先解链再删: {key}（{projects:?}）")]
    Referenced { key: String, projects: Vec<String> },
}

impl ErrorCode for SkillError {
    fn error_code(&self) -> &'static str {
        match self {
            SkillError::Io(_) => "io",
            SkillError::Git(g) => g.error_code(),
            SkillError::TomlDe(_) => "toml_parse",
            SkillError::TomlSer(_) => "toml_serialize",
            SkillError::HubNotInitialized(_) => "hub_not_initialized",
            SkillError::Hub(h) => h.error_code(),
            SkillError::Lock(l) => l.error_code(),
            SkillError::SkillNotFound { .. } => "skill_not_found",
            SkillError::AlreadyExists { .. } => "already_exists",
            SkillError::Archived { .. } => "archived",
            SkillError::VersionNotAdvancing { .. } => "version_not_advancing",
            SkillError::InvalidSource { .. } => "invalid_source",
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

/// SKILL.md frontmatter 顶层五字段：name / description / version / source / tags。
/// 其余顶层字段不解析也不写回，随内容字节透传（fsh 只替换 version 行）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Frontmatter {
    /// skill 名（= SKILL.md 所在目录名 = 唯一键末段）
    pub name: String,
    /// 一句话描述（入库必填）
    pub description: String,
    /// 发版输入：目标版本号（带引号的 x.y.z）；缺省时 add 取 0.1.0
    pub version: Option<String>,
    /// 外部来源（推荐 owner/repo 风格，不校验）；自产 skill 可缺省
    pub source: Option<String>,
    /// 自由标签（YAML 列表，流式 / 块式等价）
    pub tags: Vec<String>,
}

/// add 的结果报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddReport {
    /// 入库的 skill 唯一键（store 内相对路径）
    pub key: String,
    /// 首版版本号
    pub version: String,
}

/// list 的单行摘要
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillSummary {
    /// skill 唯一键（store 内相对路径）
    pub key: String,
    /// 最新版本（无任何版本 tag 时为 None）
    pub latest_version: Option<String>,
    /// 是否归档（有版本无工作副本）
    pub archived: bool,
    /// 入库来源
    pub source: Option<String>,
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
    /// 编辑面 frontmatter 五字段（活跃态读工作副本，归档态读最新 tag 树；
    /// 读不到或解析不出来时为 None）
    pub frontmatter: Option<Frontmatter>,
    /// 版本时间线（全部版本 tag，新在前）
    pub versions: Vec<String>,
    /// 引用此 skill 的项目路径（取自 hub 侧登记簿）
    pub referenced_by: Vec<String>,
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

/// 打开 hub 仓库；未初始化给出结构化错误
fn open_repo(hub_root: &Path) -> Result<git2::Repository, SkillError> {
    if !hub_root.join(".git").exists() {
        return Err(SkillError::HubNotInitialized(
            hub_root.display().to_string(),
        ));
    }
    Ok(git2::Repository::open(hub_root).map_err(git::GitError::from)?)
}

/// 取全局写锁并打开仓库（写操作统一入口姿势）
fn lock_and_open(hub_root: &Path) -> Result<(WriteToken, git2::Repository, HubPaths), SkillError> {
    let paths = HubPaths::from_root(hub_root);
    let guard = lock::write_lock(&paths)?;
    let repo = open_repo(hub_root)?;
    Ok((guard, repo, paths))
}

/// 提交身份：读 hub.toml；未配置时用默认身份
fn identity(paths: &HubPaths) -> Result<hub::UserIdentity, SkillError> {
    Ok(hub::load_config(paths)?.unwrap_or_default().user)
}

/// 读某键的 frontmatter：活跃态读工作副本，归档态读最新 tag 树里的 SKILL.md。
/// 读不到或解析不出来返回 None——查询面不因脏文件报错
fn frontmatter_of(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    versions: &[String],
) -> Result<Option<Frontmatter>, SkillError> {
    let label = format!("{}/SKILL.md", crate::naming::store_subpath(key));
    let text = if workdir_exists(paths, key) {
        match fs::read_to_string(paths.skill_workdir(key).join("SKILL.md")) {
            Ok(t) => t,
            // 工作副本在场却读不到 SKILL.md 是脏文件：查询面照旧返回空值，但留痕供排查
            Err(e) => {
                tracing::warn!(
                    key = %key,
                    path = %paths.skill_workdir(key).join("SKILL.md").display(),
                    cause = %e,
                    "读取 skill 元数据失败"
                );
                return Ok(None);
            }
        }
    } else {
        let Some(latest) = versions.first() else {
            return Ok(None);
        };
        match skill_md_from_tag(repo, &crate::naming::version_tag(key, latest), key)? {
            Some(t) => t,
            None => return Ok(None),
        }
    };
    // 解析失败同样是降级：查询面不因单条坏 frontmatter 中断，但必须能查出是哪个键
    match parse(&text, &label) {
        Ok(fm) => Ok(Some(fm)),
        Err(e) => {
            tracing::warn!(key = %key, cause = %e, "跳过无法解析的 skill");
            Ok(None)
        }
    }
}

/// 无效 frontmatter / 源目录的统一错误构造
pub(super) fn invalid(path: &str, reason: String) -> SkillError {
    SkillError::InvalidSource {
        path: path.to_string(),
        reason,
    }
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

/// 引用某 skill 的项目清单：只读 hub 侧登记簿（用法的唯一记录）
fn referencing_projects(paths: &HubPaths, key: &str) -> Result<Vec<String>, SkillError> {
    Ok(hub::load_registry(paths)?.referencing_projects(key))
}

/// 工作副本是否在盘
fn workdir_exists(paths: &HubPaths, key: &str) -> bool {
    paths.skill_workdir(key).is_dir()
}

/// 收集某键命名空间下的全部版本 tag 短名。归属判定走 naming 的尾段语义：
/// 多级键下，嵌套子键的引用（如 a/b 之下的 a/b/vendor/v1.0.0）不属于本键
fn skill_tags(repo: &git2::Repository, key: &str) -> Result<Vec<String>, SkillError> {
    let mut tags = Vec::new();
    repo.tag_foreach(|_oid, name| {
        let full = String::from_utf8_lossy(name);
        if crate::naming::key_from_tag_ref(&full) == Some(key) {
            tags.push(full.strip_prefix("refs/tags/").unwrap_or(&full).to_string());
        }
        true
    })?;
    Ok(tags)
}

/// 读 tag 内容树里 store/<键>/SKILL.md 的文本（归档态读 frontmatter 用）
fn skill_md_from_tag(
    repo: &git2::Repository,
    tag: &str,
    key: &str,
) -> Result<Option<String>, SkillError> {
    let (_, tree_id) = git::resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let entry = match tree.get_path(Path::new(&format!(
        "{}/SKILL.md",
        crate::naming::store_subpath(key)
    ))) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let obj = entry.to_object(repo)?;
    let blob = obj.peel_to_blob()?;
    Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
}

/// 发版二连：以提交身份收编全部改动 → 打版本 tag。
/// 首版（add）与后续版（release）共用，两条路径的发布语义永远一致。
/// 凭证参数保证两条发布路径都持全局写锁后进入。
fn publish(
    repo: &git2::Repository,
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
