//! 项目侧三件套数据模型：skills.toml 声明、skills.lock.toml 账本、
//! state.toml 本机状态；附盘上目录与 git tag 的统一内容哈希口径。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::DeployError;
use crate::git;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

/// skills.toml：人写的技能声明清单
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SkillsManifest {
    /// 每条声明一个技能
    #[serde(default)]
    pub skills: Vec<SkillReq>,
}

/// 单条技能声明
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillReq {
    /// 技能名（= store\ 下的目录名 = tag 命名空间）
    pub name: String,
    /// 版本引用：精确 semver（"1.0.0"）或 "stable"
    pub version: String,
    /// 目标工具名（adapter 阶段负责映射到具体目录）
    pub target: String,
}

/// skills.lock.toml：机器写的精确账本（可提交 git）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LockFile {
    /// 每条记录一个已部署技能的精确事实
    #[serde(default)]
    pub entries: Vec<LockEntry>,
}

/// 单条部署账目
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockEntry {
    /// 技能名
    pub name: String,
    /// 解析后的精确版本（stable 已展开）
    pub version: String,
    /// 版本 tag 指向的 commit id（hex）
    pub commit: String,
    /// 版本内容树哈希（漂移检测基准）
    pub tree_hash: String,
    /// 实际链接方式
    pub kind: LinkKind,
    /// 链接落点（相对项目根，如 .fsh/skills/demo）
    pub dst: String,
}

/// state.toml：本机链接状态（不提交）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectState {
    /// 每条一个实际存在的链接
    #[serde(default)]
    pub links: Vec<StateLink>,
}

/// 单条本机链接状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateLink {
    /// 技能名
    pub skill: String,
    /// 精确版本
    pub version: String,
    /// 链接方式
    pub kind: LinkKind,
    /// 链接落点（相对项目根）
    pub dst: String,
    /// 建立时间（Unix 秒）
    pub linked_at: u64,
}

/// 读取项目声明；文件缺失视为空声明（首次使用）
pub(crate) fn read_manifest(project: &Path) -> Result<SkillsManifest, DeployError> {
    match fs::read_to_string(HubPaths::project_manifest(project)) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SkillsManifest::default()),
        Err(e) => Err(e.into()),
    }
}

/// 读取账本；文件缺失视为空账本
pub(crate) fn read_lock(project: &Path) -> Result<LockFile, DeployError> {
    match fs::read_to_string(HubPaths::project_lock(project)) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LockFile::default()),
        Err(e) => Err(e.into()),
    }
}

/// 写账本
pub(crate) fn write_lock(project: &Path, lock: &LockFile) -> Result<(), DeployError> {
    fs::write(HubPaths::project_lock(project), toml::to_string(lock)?)?;
    Ok(())
}

/// 读取本机状态；文件缺失视为空状态
pub(crate) fn read_state(project: &Path) -> Result<ProjectState, DeployError> {
    match fs::read_to_string(HubPaths::project_state(project)) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectState::default()),
        Err(e) => Err(e.into()),
    }
}

/// 写本机状态
pub(crate) fn write_state(project: &Path, state: &ProjectState) -> Result<(), DeployError> {
    fs::write(HubPaths::project_state(project), toml::to_string(state)?)?;
    Ok(())
}

// ---------- 内部：内容哈希（盘上目录 vs git tag，逐文件内容比对基准） ----------

/// 收集「相对路径 → 文件字节」，按相对路径排序（两侧同口径，抵消遍历序差异）
fn collect_files_sorted(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, std::ffi::OsString)>,
) -> std::io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let rel = if prefix.is_empty() {
            name.to_string_lossy().to_string()
        } else {
            format!("{prefix}/{}", name.to_string_lossy())
        };
        if entry.path().is_dir() {
            collect_files_sorted(&entry.path(), &rel, out)?;
        } else if name != link::SOURCE_MANIFEST_FILE {
            // 拷贝记账是部署面元数据，不计入技能内容
            out.push((rel, name));
        }
    }
    Ok(())
}

/// 递归计算磁盘目录的内容哈希：确定性顺序，喂「相对路径 + 文件字节」。
/// .fsh-source.toml 是拷贝记账，属于部署面元数据，不计入技能内容。
pub(crate) fn hash_dir_contents(dir: &Path) -> Result<String, DeployError> {
    let mut files = Vec::new();
    collect_files_sorted(dir, "", &mut files)?;
    let mut hasher = blake3::Hasher::new();
    for (rel, _) in &files {
        hasher.update(rel.as_bytes());
        hasher.update(&fs::read(dir.join(rel))?);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// 计算 git 版本 tag 子树的内容哈希（与 hash_dir_contents 同一口径，
/// 喂「相对路径 + blob 字节」），供磁盘比对。
pub(crate) fn hash_tag_contents(
    repo: &git2::Repository,
    tag: &str,
    subpath: &str,
) -> Result<String, DeployError> {
    let (_, tree_id) = git::resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let subtree = tree.get_path(Path::new(subpath))?;
    let subtree = subtree.to_object(repo)?.peel_to_tree()?;

    // 先序遍历收集 (相对路径, blob id)，再排序，与磁盘侧同口径
    let mut files: Vec<(String, git2::Oid)> = Vec::new();
    subtree.walk(
        git2::TreeWalkMode::PreOrder,
        |dir: &str, entry: &git2::TreeEntry<'_>| {
            if let Some(git2::ObjectType::Blob) = entry.kind() {
                let rel = format!("{dir}{}", entry.name().unwrap_or_default());
                files.push((rel, entry.id()));
            }
            true
        },
    )?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = blake3::Hasher::new();
    for (rel, oid) in &files {
        hasher.update(rel.as_bytes());
        hasher.update(repo.find_blob(*oid)?.content());
    }
    Ok(hasher.finalize().to_hex().to_string())
}
