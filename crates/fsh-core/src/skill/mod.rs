//! 库管理：skill 的入库（add）、发版（version bump）、两级删除（归档 /
//! purge）、恢复（restore）与查询（list / show / diff）。
//! 版本唯一实体是 git tag；工作副本是编辑面，物化目录是部署面——
//! 归档只删编辑面，版本链（tag）与部署面完好。
//! 全部写操作先取进程级文件锁（见 lock 模块），CLI 与 GUI 共用。
//! 本模块为目录模块：add / version / remove / restore / query 五个操作
//! 子模块共享此处的错误类型、公共类型与内部工具，对外 API 统一经此处重导出发布。

mod add;
mod query;
mod remove;
mod restore;
mod version;

// ---------- 公共 API ----------
pub use add::{add, add_dry};
pub use query::{diff, list, show};
pub use remove::{remove, remove_dry};
pub use restore::{restore, restore_dry};
pub use version::{bump_dry, version_bump};

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::deploy;
use crate::git;
use crate::hub;
use crate::lock::{self, LockGuard};
use crate::paths::HubPaths;

/// 库管理错误
#[derive(Debug, Error)]
pub enum SkillError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// meta / 配置 TOML 解析失败
    #[error("TOML 解析失败: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// meta / 配置 TOML 序列化失败
    #[error("TOML 序列化失败: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// hub 未初始化（无 .git）
    #[error("hub 未初始化: {0}")]
    HubNotInitialized(String),
    /// 登记簿 / 配置读写失败
    #[error("hub 配置错误: {0}")]
    Hub(#[from] hub::HubError),
    /// 项目账本读取失败（引用查找）
    #[error("部署读取错误: {0}")]
    Deploy(#[from] deploy::DeployError),
    /// 文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] lock::LockError),
    /// skill 不存在（无工作副本也无版本）
    #[error("skill 不存在: {skill}")]
    SkillNotFound { skill: String },
    /// 同名 skill 已存在（工作副本或版本 tag 占位）
    #[error("skill 已存在: {skill}")]
    AlreadyExists { skill: String },
    /// skill 处于归档态，该操作只对活跃 skill 开放
    #[error("skill 已归档: {skill}")]
    Archived { skill: String },
    /// 源目录不满足入库条件
    #[error("无效的 skill 源: {path}（{reason}）")]
    InvalidSource { path: String, reason: String },
    /// purge 被项目引用拒绝（附引用方项目路径）
    #[error("skill 仍被项目引用，先解链再删: {skill}（{projects:?}）")]
    Referenced {
        skill: String,
        projects: Vec<String>,
    },
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
            SkillError::Deploy(d) => d.error_code(),
            SkillError::Lock(l) => l.error_code(),
            SkillError::SkillNotFound { .. } => "skill_not_found",
            SkillError::AlreadyExists { .. } => "already_exists",
            SkillError::Archived { .. } => "archived",
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

/// SKILL.md frontmatter：skill 的身份来源（name 必填，其余可选）
#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter {
    /// skill 名（= store 目录名 = tag 命名空间）
    pub name: String,
    /// 指定首版版本号（缺省 0.1.0）
    pub version: Option<String>,
    /// 一句话描述
    pub description: Option<String>,
}

/// meta\\<skill>.toml：skill 元数据（tracked）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SkillMeta {
    /// 入库来源（首次 add 时的源目录）
    pub source: String,
    /// 自由标签（list --tag 过滤用）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 作者
    #[serde(default)]
    pub author: Option<String>,
    /// 许可证
    #[serde(default)]
    pub license: Option<String>,
}

/// add 的结果报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddReport {
    /// 入库的 skill 名
    pub skill: String,
    /// 首版版本号
    pub version: String,
}

/// remove 的结果报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoveReport {
    /// skill 名
    pub skill: String,
    /// 是否彻底删除（连版本 tag 一起）
    pub purged: bool,
}

/// list 的单行摘要
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillSummary {
    /// skill 名
    pub name: String,
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
    /// skill 名
    pub name: String,
    /// 最新版本
    pub latest_version: Option<String>,
    /// 是否归档
    pub archived: bool,
    /// 元数据（meta 文件缺失时为 None）
    pub meta: Option<SkillMeta>,
    /// 版本时间线（全部版本 tag，新在前）
    pub versions: Vec<String>,
    /// frontmatter 描述
    pub description: Option<String>,
    /// 引用此 skill 的项目路径（登记簿 + 各项目 lock）
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

/// semver 段位（version --bump 的目标段）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpPart {
    /// 补丁位 x.y.Z
    Patch,
    /// 次版本位 x.Y.0
    Minor,
    /// 主版本位 X.0.0
    Major,
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
fn lock_and_open(hub_root: &Path) -> Result<(LockGuard, git2::Repository, HubPaths), SkillError> {
    let paths = HubPaths::from_root(hub_root);
    let guard = lock::write_lock(&paths)?;
    let repo = open_repo(hub_root)?;
    Ok((guard, repo, paths))
}

/// 提交身份：读 hub.toml；未配置时用默认身份
fn identity(paths: &HubPaths) -> Result<hub::UserIdentity, SkillError> {
    Ok(hub::load_config(paths)?.unwrap_or_default().user)
}

/// meta 文件路径
fn meta_path(paths: &HubPaths, skill: &str) -> PathBuf {
    paths.meta().join(format!("{skill}.toml"))
}

/// 读 meta；文件缺失返回 None
fn load_meta(paths: &HubPaths, skill: &str) -> Result<Option<SkillMeta>, SkillError> {
    match fs::read_to_string(meta_path(paths, skill)) {
        Ok(text) => Ok(Some(toml::from_str(&text)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 解析 SKILL.md frontmatter：首行 --- 到下一个 --- 之间的扁平 `key: value`。
// ponytail: 只解析扁平键值，不引入 YAML 依赖；嵌套结构超出 skill 声明需求
fn parse_frontmatter(text: &str) -> Option<BTreeMap<String, String>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut map = BTreeMap::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            return Some(map);
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    None
}

/// 从文本读出 skill 身份；frontmatter 缺失或无 name 视为无效
fn skill_identity(text: &str) -> Result<Frontmatter, SkillError> {
    let fm = parse_frontmatter(text).ok_or_else(|| SkillError::InvalidSource {
        path: "SKILL.md".into(),
        reason: "缺少 frontmatter（--- 包围的头部区）".into(),
    })?;
    let name = fm.get("name").cloned().filter(|n| !n.is_empty());
    let name = name.ok_or_else(|| SkillError::InvalidSource {
        path: "SKILL.md".into(),
        reason: "frontmatter 缺少 name 字段".into(),
    })?;
    Ok(Frontmatter {
        name,
        version: fm.get("version").cloned(),
        description: fm.get("description").cloned(),
    })
}

/// 校验 x.y.z 三段纯数字版本号
fn valid_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u64>().is_ok())
}

/// 版本号段位自增：x.y.z 按指定位 +1，低位归零
fn bump_semver(v: &str, part: BumpPart) -> Result<String, SkillError> {
    if !valid_semver(v) {
        return Err(SkillError::InvalidSource {
            path: "版本号".into(),
            reason: format!("不是合法的 x.y.z: {v}"),
        });
    }
    let mut seg: Vec<u64> = v.split('.').map(|p| p.parse().unwrap()).collect();
    match part {
        BumpPart::Patch => seg[2] += 1,
        BumpPart::Minor => {
            seg[1] += 1;
            seg[2] = 0;
        }
        BumpPart::Major => {
            seg[0] += 1;
            seg[1] = 0;
            seg[2] = 0;
        }
    }
    Ok(seg
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("."))
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

/// 引用某 skill 的项目清单：走引用集（refs 模块）取引用方，
/// 不可达项目一律不贡献引用——与 GC / doctor 同一口径
fn referencing_projects(paths: &HubPaths, skill: &str) -> Result<Vec<String>, SkillError> {
    Ok(crate::refs::reference_set(paths)?.referencing_projects(skill))
}

/// 工作副本是否在盘
fn workdir_exists(paths: &HubPaths, skill: &str) -> bool {
    paths.skill_workdir(skill).is_dir()
}

/// 收集某 skill 命名空间下的全部 tag 短名（版本 tag + stable 指针）
fn skill_tags(repo: &git2::Repository, skill: &str) -> Result<Vec<String>, SkillError> {
    let prefix = format!("{skill}/");
    let mut tags = Vec::new();
    repo.tag_foreach(|_oid, name| {
        let short = String::from_utf8_lossy(name);
        if let Some(rest) = short.strip_prefix("refs/tags/")
            && rest.starts_with(&prefix)
        {
            tags.push(rest.to_string());
        }
        true
    })?;
    Ok(tags)
}

/// 读 tag 内容树里 store/<skill>/SKILL.md 的文本（归档态读 frontmatter 用）
fn skill_md_from_tag(
    repo: &git2::Repository,
    tag: &str,
    skill: &str,
) -> Result<Option<String>, SkillError> {
    let (_, tree_id) = git::resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let entry = match tree.get_path(Path::new(&format!("store/{skill}/SKILL.md"))) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let obj = entry.to_object(repo)?;
    let blob = obj.peel_to_blob()?;
    Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
}

/// 发版三连：以提交身份收编全部改动 → 打版本 tag → 物化到部署面。
/// 首版（add）与后续版（version --bump）共用，两条路径的发布语义永远一致。
fn release(
    repo: &git2::Repository,
    paths: &HubPaths,
    skill: &str,
    version: &str,
    message: &str,
) -> Result<(), SkillError> {
    let id = identity(paths)?;
    git::commit_all(repo, message, &id.name, &id.email)?;
    git::tag_version(repo, skill, version)?;
    git::materialize(
        repo,
        &format!("{skill}/v{version}"),
        &format!("store/{skill}"),
        &paths.materialized(skill, version),
    )?;
    Ok(())
}
