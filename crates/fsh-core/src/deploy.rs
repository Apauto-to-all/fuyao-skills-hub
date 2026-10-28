//! 部署引擎：plan（纯计算变更清单）→ apply（物化 + 链接 + 记账 + 顺手清）
//! → status（双侧漂移检测）。项目侧三件套：skills.toml（人写声明）、
//! skills.lock.toml（机器账本，可提交）、state.json（本机链接状态，不提交）。
//! 版本引用只支持精确 semver 与 stable（stable = 最新版本 tag）。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::git;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

/// 部署引擎错误
#[derive(Debug, Error)]
pub enum DeployError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// TOML 解析失败（skills.toml / skills.lock.toml）
    #[error("TOML 解析失败: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// TOML 序列化失败
    #[error("TOML 序列化失败: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// JSON 序列化失败（state.json）
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// hub.toml 缺失或损坏（hub 未初始化）
    #[error("hub 未初始化: {0}")]
    HubNotInitialized(String),
    /// 声明里引用的版本在 hub 中不存在
    #[error("版本不存在: {skill}@{version}")]
    VersionNotFound { skill: String, version: String },
    /// 链接操作失败
    #[error("链接错误: {0}")]
    Link(#[from] link::LinkError),
}

// git2 原生错误统一收口到 Git 变体，使 `?` 在全部 git2 调用点直接可用
impl From<git2::Error> for DeployError {
    fn from(e: git2::Error) -> Self {
        DeployError::Git(git::GitError::from(e))
    }
}

// ---------- 项目侧三件套的数据模型 ----------

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

/// state.json：本机链接状态（不提交）
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

// ---------- plan 的输出：变更清单（可序列化，GUI 直接渲染） ----------

/// 单条计划变更
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Change {
    /// 新装：hub 有此技能此版本，项目还没链
    Install {
        /// 技能名
        name: String,
        /// 解析后的精确版本
        version: String,
        /// 目标工具名
        target: String,
    },
    /// 升级到更高版本
    Upgrade {
        /// 技能名
        name: String,
        /// 旧版本
        from: String,
        /// 新版本
        to: String,
        /// 目标工具名
        target: String,
    },
    /// 降级到更低版本
    Downgrade {
        /// 技能名
        name: String,
        /// 旧版本
        from: String,
        /// 新版本
        to: String,
        /// 目标工具名
        target: String,
    },
    /// 声明中移除：撤销链接并销账
    Remove {
        /// 技能名
        name: String,
        /// 将被撤销的版本
        version: String,
    },
    /// 已满足：无需任何动作
    Noop {
        /// 技能名
        name: String,
        /// 已就位的版本
        version: String,
    },
}

// ---------- status 的输出：漂移清单（可序列化） ----------

/// 单条漂移发现
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "drift", rename_all = "snake_case")]
pub enum Drift {
    /// 项目侧：链接落点不存在、悬空，或被外部重指到别的目录
    LinkMissing {
        /// 技能名
        name: String,
    },
    /// 项目侧：copy 模式内容被改动（哈希与账本不符）
    CopyModified {
        /// 技能名
        name: String,
    },
    /// 项目侧：账本引用的版本在 hub 中已不存在
    VersionGone {
        /// 技能名
        name: String,
        /// 消失的版本
        version: String,
    },
    /// hub 侧：工作副本有未提交改动
    HubDirty,
    /// hub 侧：物化目录内容与版本 tag 不符
    MaterialMismatch {
        /// 技能名
        name: String,
        /// 受疑版本
        version: String,
    },
}

// ---------- 公共 API ----------

/// 打开 hub 仓库；未初始化时给出结构化错误
fn open_repo(hub_root: &Path) -> Result<git2::Repository, DeployError> {
    if !hub_root.join(".git").exists() {
        return Err(DeployError::HubNotInitialized(
            hub_root.display().to_string(),
        ));
    }
    Ok(git2::Repository::open(hub_root).map_err(git::GitError::from)?)
}

/// 读取项目声明；文件缺失视为空声明（首次使用）
fn read_manifest(project: &Path) -> Result<SkillsManifest, DeployError> {
    match fs::read_to_string(HubPaths::project_manifest(project)) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SkillsManifest::default()),
        Err(e) => Err(e.into()),
    }
}

/// 读取账本；文件缺失视为空账本
fn read_lock(project: &Path) -> Result<LockFile, DeployError> {
    match fs::read_to_string(HubPaths::project_lock(project)) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LockFile::default()),
        Err(e) => Err(e.into()),
    }
}

/// 写账本
fn write_lock(project: &Path, lock: &LockFile) -> Result<(), DeployError> {
    fs::write(HubPaths::project_lock(project), toml::to_string(lock)?)?;
    Ok(())
}

/// 读取本机状态；文件缺失视为空状态
fn read_state(project: &Path) -> Result<ProjectState, DeployError> {
    match fs::read_to_string(HubPaths::project_state(project)) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectState::default()),
        Err(e) => Err(e.into()),
    }
}

/// 写本机状态
fn write_state(project: &Path, state: &ProjectState) -> Result<(), DeployError> {
    fs::write(
        HubPaths::project_state(project),
        serde_json::to_string_pretty(state)?,
    )?;
    Ok(())
}

/// 把声明里的版本引用解析为精确版本：stable → 最新版本 tag；
/// 精确版本需在 hub 中真实存在
fn resolve_version(repo: &git2::Repository, req: &SkillReq) -> Result<String, DeployError> {
    if req.version == "stable" {
        return git::tag_list(repo, &req.name)?.into_iter().next().ok_or(
            DeployError::VersionNotFound {
                skill: req.name.clone(),
                version: req.version.clone(),
            },
        );
    }
    let tag = format!("{}/v{}", req.name, req.version);
    match git::resolve(repo, &tag) {
        Ok(_) => Ok(req.version.clone()),
        Err(git::GitError::TagNotFound { .. }) => Err(DeployError::VersionNotFound {
            skill: req.name.clone(),
            version: req.version.clone(),
        }),
        Err(e) => Err(e.into()),
    }
}

/// 计算变更清单：对比 skills.toml 声明、skills.lock.toml 账本与 hub 现状。
/// 纯计算，不产生任何磁盘副作用。
pub fn plan(hub_root: &Path, project_dir: &Path) -> Result<Vec<Change>, DeployError> {
    let repo = open_repo(hub_root)?;
    let manifest = read_manifest(project_dir)?;
    let lock = read_lock(project_dir)?;

    let mut changes = Vec::new();
    // 声明侧：逐条对比账本
    for req in &manifest.skills {
        let version = resolve_version(&repo, req)?;
        match lock.entries.iter().find(|e| e.name == req.name) {
            None => changes.push(Change::Install {
                name: req.name.clone(),
                version,
                target: req.target.clone(),
            }),
            Some(entry) => {
                if entry.version == version {
                    changes.push(Change::Noop {
                        name: req.name.clone(),
                        version,
                    });
                } else {
                    // 新旧版本在 tag 降序表中的位置决定升降级语义
                    let order = git::tag_list(&repo, &req.name)?;
                    let from_pos = order.iter().position(|v| v == &entry.version);
                    let to_pos = order.iter().position(|v| v == &version);
                    let is_upgrade = match (from_pos, to_pos) {
                        (Some(f), Some(t)) => t < f,
                        // 位置对不上时按升级处理（账本版本可能已不在 hub）
                        _ => true,
                    };
                    let fields = (
                        req.name.clone(),
                        entry.version.clone(),
                        version,
                        req.target.clone(),
                    );
                    changes.push(if is_upgrade {
                        Change::Upgrade {
                            name: fields.0,
                            from: fields.1,
                            to: fields.2,
                            target: fields.3,
                        }
                    } else {
                        Change::Downgrade {
                            name: fields.0,
                            from: fields.1,
                            to: fields.2,
                            target: fields.3,
                        }
                    });
                }
            }
        }
    }
    // 账本侧：声明里没有的技能要撤销
    for entry in &lock.entries {
        if !manifest.skills.iter().any(|r| r.name == entry.name) {
            changes.push(Change::Remove {
                name: entry.name.clone(),
                version: entry.version.clone(),
            });
        }
    }
    Ok(changes)
}

/// 落点相对项目根的统一字符串形式（正斜杠，跨工具可读）
fn rel_dst(dst: &Path, project_dir: &Path) -> String {
    dst.strip_prefix(project_dir)
        .unwrap_or(dst)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 执行 plan 产出的全部变更：物化 + 建链 + 写账本/状态，末尾顺手清
/// 本项目已不再引用且内容与 tag 一致的物化目录。
// TODO: M1 全部写操作与 hub 写入口共用进程级文件锁
pub fn apply(hub_root: &Path, project_dir: &Path) -> Result<Vec<Change>, DeployError> {
    let repo = open_repo(hub_root)?;
    let changes = plan(hub_root, project_dir)?;
    let install_dir = hub_root
        .parent()
        .ok_or_else(|| DeployError::HubNotInitialized(hub_root.display().to_string()))?;
    let paths = HubPaths::new(install_dir);

    let mut lock = read_lock(project_dir)?;
    let mut state = read_state(project_dir)?;

    for change in &changes {
        // Install / Upgrade / Downgrade 的落盘动作相同：物化 → 链接 → 记账；
        // Noop 也要自愈——部署面（物化目录/链接）可能被删，账本对不上盘就补
        let (name, version, is_noop) = match change {
            Change::Install { name, version, .. }
            | Change::Upgrade {
                name, to: version, ..
            }
            | Change::Downgrade {
                name, to: version, ..
            } => (name, version, false),
            Change::Noop { name, version } => (name, version, true),
            Change::Remove { name, version } => {
                link::remove_link(&HubPaths::project_link_dst(project_dir, name))?;
                lock.entries.retain(|e| &e.name != name);
                state.links.retain(|l| &l.skill != name);
                let _ = version; // 账目按技能名清除，版本仅用于变更报告
                continue;
            }
        };

        let tag = format!("{name}/v{version}");
        let subpath = format!("store/{name}");
        let mat = paths.materialized(name, version);
        // 物化目录缺失或内容与 tag 不符时重物化（可删可重建是部署面的天性）
        if !mat.is_dir() || hash_dir_contents(&mat)? != hash_tag_contents(&repo, &tag, &subpath)? {
            if mat.exists() {
                fs::remove_dir_all(&mat)?;
            }
            git::materialize(&repo, &tag, &subpath, &mat)?;
        }
        let content_hash = hash_tag_contents(&repo, &tag, &subpath)?;

        // 链接落点：新装/升降级先清旧链再建新链；Noop 只在链接在位
        // 且指向仍是本次物化目录时续用（copy 无指向概念，在位即续用）
        let dst = HubPaths::project_link_dst(project_dir, name);
        let link_present = dst.symlink_metadata().is_ok() && dst.exists();
        let recorded_kind = lock
            .entries
            .iter()
            .find(|e| &e.name == name)
            .map(|e| e.kind);
        let points_to_mat =
            matches!((dst.canonicalize(), mat.canonicalize()), (Ok(a), Ok(b)) if a == b);
        let kind = if is_noop
            && link_present
            && (recorded_kind == Some(LinkKind::Copy) || points_to_mat)
        {
            recorded_kind.unwrap_or(LinkKind::Junction)
        } else {
            link::remove_link(&dst)?;
            link::create_dir_link(&mat, &dst, &content_hash)?
        };

        let (commit, _) = git::resolve(&repo, &tag)?;
        lock.entries.retain(|e| &e.name != name);
        lock.entries.push(LockEntry {
            name: name.clone(),
            version: version.clone(),
            commit: commit.to_string(),
            tree_hash: content_hash,
            kind,
            dst: rel_dst(&dst, project_dir),
        });
        state.links.retain(|l| &l.skill != name);
        state.links.push(StateLink {
            skill: name.clone(),
            version: version.clone(),
            kind,
            dst: rel_dst(&dst, project_dir),
            linked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
    }

    write_lock(project_dir, &lock)?;
    write_state(project_dir, &state)?;

    // 顺手清：本轮触达的技能中，本项目账本已不引用且内容与 tag 一致的
    // 物化目录当场删；hash 对不上的东西绝不销毁（可能是未入库的手改）。
    // ponytail: M0 引用判定只看本项目账本；跨项目引用账本等项目登记
    // 落地后接入，届时误删由「缺失重物化 + status」兜底
    let mut touched: Vec<String> = Vec::new();
    for change in &changes {
        match change {
            Change::Install { name, .. }
            | Change::Upgrade { name, .. }
            | Change::Downgrade { name, .. }
            | Change::Remove { name, .. } => {
                if !touched.contains(name) {
                    touched.push(name.clone());
                }
            }
            Change::Noop { .. } => {}
        }
    }
    for name in touched {
        let skill_store = paths.store().join(&name);
        let Ok(entries) = fs::read_dir(&skill_store) else {
            continue;
        };
        for entry in entries.flatten() {
            // 只把「三段数字」目录当版本目录，工作副本子目录（如 refs/）不碰
            let fname = entry.file_name().to_string_lossy().to_string();
            if !entry.path().is_dir() {
                continue;
            }
            let parts: Vec<&str> = fname.split('.').collect();
            if parts.len() != 3 || parts.iter().any(|p| p.parse::<u32>().is_err()) {
                continue;
            }
            let referenced = lock
                .entries
                .iter()
                .any(|e| e.name == name && e.version == fname);
            if referenced {
                continue;
            }
            let tag = format!("{name}/v{fname}");
            let subpath = format!("store/{name}");
            // 内容与 tag 不符（或 tag 已消失）的一律跳过不删
            let Ok(tag_hash) = hash_tag_contents(&repo, &tag, &subpath) else {
                continue;
            };
            if hash_dir_contents(&entry.path()).ok().as_deref() == Some(tag_hash.as_str()) {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    Ok(changes)
}

/// 双侧漂移检测：项目侧（链接/copy 哈希/版本存在性）+ hub 侧（工作副本/
/// 物化目录）。零漂移返回空表。
pub fn status(hub_root: &Path, project_dir: &Path) -> Result<Vec<Drift>, DeployError> {
    let repo = open_repo(hub_root)?;
    let lock = read_lock(project_dir)?;
    let mut drifts = Vec::new();

    for entry in &lock.entries {
        // 项目侧：链接存在且可达
        let dst = project_dir.join(&entry.dst);
        let present = fs::symlink_metadata(&dst).is_ok() && dst.exists();
        if !present {
            drifts.push(Drift::LinkMissing {
                name: entry.name.clone(),
            });
            continue;
        }
        // 项目侧：copy 模式内容被改
        if entry.kind == LinkKind::Copy
            && hash_dir_contents(&dst).ok().as_deref() != Some(entry.tree_hash.as_str())
        {
            drifts.push(Drift::CopyModified {
                name: entry.name.clone(),
            });
        }
    }

    // hub 侧：工作副本脏
    if git::is_dirty(&repo)? {
        drifts.push(Drift::HubDirty);
    }

    let install_dir = hub_root
        .parent()
        .ok_or_else(|| DeployError::HubNotInitialized(hub_root.display().to_string()))?;
    let paths = HubPaths::new(install_dir);
    for entry in &lock.entries {
        let tag = format!("{}/v{}", entry.name, entry.version);
        // 项目侧补充：账本版本在 hub 中已消失
        let version_exists = !matches!(
            git::resolve(&repo, &tag),
            Err(git::GitError::TagNotFound { .. })
        );
        if !version_exists {
            drifts.push(Drift::VersionGone {
                name: entry.name.clone(),
                version: entry.version.clone(),
            });
            continue;
        }
        // hub 侧：物化目录缺失或与 tag 不符
        let mat = paths.materialized(&entry.name, &entry.version);
        // 项目侧补充：reparse 链接（symlink/junction）被外部重指到
        // 别的目录——copy 有 hash 兜底，链接模式的指向必须显式校验
        if entry.kind != LinkKind::Copy {
            let dst = project_dir.join(&entry.dst);
            if matches!((dst.canonicalize(), mat.canonicalize()), (Ok(a), Ok(b)) if a != b) {
                drifts.push(Drift::LinkMissing {
                    name: entry.name.clone(),
                });
            }
        }
        let tag_hash = hash_tag_contents(&repo, &tag, &format!("store/{}", entry.name))?;
        if !mat.is_dir() || hash_dir_contents(&mat).ok().as_deref() != Some(tag_hash.as_str()) {
            drifts.push(Drift::MaterialMismatch {
                name: entry.name.clone(),
                version: entry.version.clone(),
            });
        }
    }
    Ok(drifts)
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
/// .fsh-source.json 是拷贝记账，属于部署面元数据，不计入技能内容。
fn hash_dir_contents(dir: &Path) -> Result<String, DeployError> {
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
fn hash_tag_contents(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 测试用 hub + 项目：hub 初始化完毕并入库一个 demo 技能两个版本
    fn setup_hub_with_skill() -> (tempfile::TempDir, tempfile::TempDir) {
        let install = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();

        // 工作副本两个版本演进：1.0.0 → 2.0.0
        let work = paths.skill_workdir("demo");
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("SKILL.md"), "# v1\n").unwrap();
        let repo = git2::Repository::open(paths.root()).unwrap();
        git::commit_all(&repo, "demo v1", "fuyao", "fuyao@local").unwrap();
        git::tag_version(&repo, "demo", "1.0.0").unwrap();
        fs::write(work.join("SKILL.md"), "# v2\n").unwrap();
        git::commit_all(&repo, "demo v2", "fuyao", "fuyao@local").unwrap();
        git::tag_version(&repo, "demo", "2.0.0").unwrap();
        (install, project)
    }

    /// 写项目侧 skills.toml
    fn write_manifest(project: &Path, reqs: &[SkillReq]) {
        let m = SkillsManifest {
            skills: reqs.to_vec(),
        };
        fs::write(
            HubPaths::project_manifest(project),
            toml::to_string(&m).unwrap(),
        )
        .unwrap();
    }

    fn req(name: &str, version: &str) -> SkillReq {
        SkillReq {
            name: name.into(),
            version: version.into(),
            target: "claude-code".into(),
        }
    }

    fn lock_of(project: &Path) -> LockFile {
        toml::from_str(&fs::read_to_string(HubPaths::project_lock(project)).unwrap()).unwrap()
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn plan_covers_all_five_change_kinds() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();

        // 空声明 + 空账本：无变更
        assert!(plan(&hub_root, project.path()).unwrap().is_empty());

        // 新装
        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        let changes = plan(&hub_root, project.path()).unwrap();
        assert_eq!(
            changes,
            vec![Change::Install {
                name: "demo".into(),
                version: "1.0.0".into(),
                target: "claude-code".into()
            }]
        );

        // 手工构造账本模拟「已装 1.0.0」
        let lock = LockFile {
            entries: vec![LockEntry {
                name: "demo".into(),
                version: "1.0.0".into(),
                commit: "0".repeat(40),
                tree_hash: "x".into(),
                kind: LinkKind::Junction,
                dst: ".fsh/skills/demo".into(),
            }],
        };
        fs::write(
            HubPaths::project_lock(project.path()),
            toml::to_string(&lock).unwrap(),
        )
        .unwrap();

        // 已满足
        assert_eq!(
            plan(&hub_root, project.path()).unwrap(),
            vec![Change::Noop {
                name: "demo".into(),
                version: "1.0.0".into()
            }]
        );
        // 升级
        write_manifest(project.path(), &[req("demo", "2.0.0")]);
        assert_eq!(
            plan(&hub_root, project.path()).unwrap(),
            vec![Change::Upgrade {
                name: "demo".into(),
                from: "1.0.0".into(),
                to: "2.0.0".into(),
                target: "claude-code".into()
            }]
        );
        // 降级：账本视为已装 2.0.0，声明要 1.0.0
        let mut lock2 = lock.clone();
        lock2.entries[0].version = "2.0.0".into();
        fs::write(
            HubPaths::project_lock(project.path()),
            toml::to_string(&lock2).unwrap(),
        )
        .unwrap();
        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        assert_eq!(
            plan(&hub_root, project.path()).unwrap(),
            vec![Change::Downgrade {
                name: "demo".into(),
                from: "2.0.0".into(),
                to: "1.0.0".into(),
                target: "claude-code".into()
            }]
        );
        // 撤销：声明清空但账本有
        write_manifest(project.path(), &[]);
        assert_eq!(
            plan(&hub_root, project.path()).unwrap(),
            vec![Change::Remove {
                name: "demo".into(),
                version: "2.0.0".into()
            }]
        );
        // plan 全程不动盘：账本没被改写
        assert_eq!(lock_of(project.path()), lock2);
    }

    #[test]
    fn plan_resolves_stable_to_newest() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        write_manifest(project.path(), &[req("demo", "stable")]);
        let changes = plan(&hub_root, project.path()).unwrap();
        assert_eq!(
            changes,
            vec![Change::Install {
                name: "demo".into(),
                version: "2.0.0".into(),
                target: "claude-code".into()
            }]
        );
    }

    #[test]
    fn plan_reports_missing_version() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        write_manifest(project.path(), &[req("demo", "9.9.9")]);
        assert!(matches!(
            plan(&hub_root, project.path()),
            Err(DeployError::VersionNotFound { .. })
        ));
    }

    #[test]
    fn apply_installs_materializes_links_and_records() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        write_manifest(project.path(), &[req("demo", "1.0.0")]);

        let applied = apply(&hub_root, project.path()).unwrap();
        assert_eq!(applied.len(), 1);
        assert!(matches!(applied[0], Change::Install { .. }));

        // 物化目录存在且内容正确（剥子树前缀）
        let paths = HubPaths::new(install.path());
        let mat = paths.materialized("demo", "1.0.0");
        assert_eq!(fs::read_to_string(mat.join("SKILL.md")).unwrap(), "# v1\n");
        // 链接落点可读
        let dst = HubPaths::project_link_dst(project.path(), "demo");
        assert!(dst.join("SKILL.md").is_file());
        // 账本记满四要素
        let lock = lock_of(project.path());
        assert_eq!(lock.entries.len(), 1);
        let e = &lock.entries[0];
        assert_eq!(e.name, "demo");
        assert_eq!(e.version, "1.0.0");
        assert_ne!(e.commit, "0".repeat(40), "应记真实 commit id");
        assert!(!e.tree_hash.is_empty());
        assert_eq!(e.dst, ".fsh/skills/demo");
        // state.json 有链接记录
        let state: ProjectState = serde_json::from_str(
            &fs::read_to_string(HubPaths::project_state(project.path())).unwrap(),
        )
        .unwrap();
        assert_eq!(state.links.len(), 1);
        assert!(state.links[0].linked_at <= now());

        // 幂等：重跑全是 Noop 且零副作用
        let again = apply(&hub_root, project.path()).unwrap();
        assert_eq!(
            again,
            vec![Change::Noop {
                name: "demo".into(),
                version: "1.0.0".into()
            }]
        );
        assert_eq!(lock_of(project.path()).entries.len(), 1);

        // apply 后 status 零漂移
        assert!(status(&hub_root, project.path()).unwrap().is_empty());
    }

    #[test]
    fn apply_upgrade_relinks_and_gc_sweeps_old() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        let paths = HubPaths::new(install.path());

        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        apply(&hub_root, project.path()).unwrap();
        assert!(paths.materialized("demo", "1.0.0").is_dir());

        // 升到 2.0.0：旧物化目录被顺手清（本项目无引用、内容与 tag 一致）
        write_manifest(project.path(), &[req("demo", "2.0.0")]);
        let applied = apply(&hub_root, project.path()).unwrap();
        assert!(matches!(applied[0], Change::Upgrade { .. }));
        assert!(
            !paths.materialized("demo", "1.0.0").exists(),
            "无引用旧物化应被清"
        );
        assert!(paths.materialized("demo", "2.0.0").is_dir());
        // 链接指向新版本内容
        let dst = HubPaths::project_link_dst(project.path(), "demo");
        assert!(dst.join("SKILL.md").is_file());
        assert_eq!(lock_of(project.path()).entries[0].version, "2.0.0");
        assert!(status(&hub_root, project.path()).unwrap().is_empty());
    }

    #[test]
    fn apply_rematerializes_when_dir_missing() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        let paths = HubPaths::new(install.path());

        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        apply(&hub_root, project.path()).unwrap();
        // 人为删掉物化目录（模拟 GC / git clean）
        fs::remove_dir_all(paths.materialized("demo", "1.0.0")).unwrap();
        // status 报物化缺失（链接悬空随之暴露，两条都是事实）
        let drifts = status(&hub_root, project.path()).unwrap();
        assert!(
            drifts.contains(&Drift::MaterialMismatch {
                name: "demo".into(),
                version: "1.0.0".into()
            }),
            "应检出物化缺失: {drifts:?}"
        );
        // apply 自动重物化并归位
        apply(&hub_root, project.path()).unwrap();
        assert!(paths.materialized("demo", "1.0.0").is_dir());
        assert!(status(&hub_root, project.path()).unwrap().is_empty());
    }

    #[test]
    fn apply_remove_unlinks_and_cleans_entry() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        apply(&hub_root, project.path()).unwrap();

        write_manifest(project.path(), &[]);
        let applied = apply(&hub_root, project.path()).unwrap();
        assert!(matches!(applied[0], Change::Remove { .. }));
        assert!(
            !HubPaths::project_link_dst(project.path(), "demo").exists(),
            "链接应撤销"
        );
        assert!(lock_of(project.path()).entries.is_empty(), "账目应清除");
        let paths = HubPaths::new(install.path());
        assert!(
            !paths.materialized("demo", "1.0.0").exists(),
            "无引用物化应被顺手清"
        );
    }

    #[test]
    fn status_detects_project_side_drifts() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        apply(&hub_root, project.path()).unwrap();
        let dst = HubPaths::project_link_dst(project.path(), "demo");

        // 链接被删
        let meta = fs::symlink_metadata(&dst).unwrap();
        if meta.file_type().is_symlink() {
            fs::remove_dir(&dst).unwrap();
        } else {
            fs::remove_dir_all(&dst).unwrap();
        }
        assert_eq!(
            status(&hub_root, project.path()).unwrap(),
            vec![Drift::LinkMissing {
                name: "demo".into()
            }]
        );

        // 恢复后 copy 模式篡改内容：只有拷贝才能篡改；非拷贝模式跳过此断言
        apply(&hub_root, project.path()).unwrap();
        if lock_of(project.path()).entries[0].kind == LinkKind::Copy {
            fs::write(dst.join("SKILL.md"), "tampered\n").unwrap();
            assert_eq!(
                status(&hub_root, project.path()).unwrap(),
                vec![Drift::CopyModified {
                    name: "demo".into()
                }]
            );
        }
    }

    #[test]
    fn status_detects_hub_side_dirty() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        apply(&hub_root, project.path()).unwrap();

        // 弄脏工作副本
        let paths = HubPaths::new(install.path());
        fs::write(paths.skill_workdir("demo").join("SKILL.md"), "hand edit\n").unwrap();
        let drifts = status(&hub_root, project.path()).unwrap();
        assert!(
            drifts.contains(&Drift::HubDirty),
            "应检出 HubDirty: {drifts:?}"
        );
    }

    #[test]
    fn status_and_apply_heal_relinked_pointing_elsewhere() {
        let (install, project) = setup_hub_with_skill();
        let hub_root = HubPaths::new(install.path()).root().to_path_buf();
        write_manifest(project.path(), &[req("demo", "1.0.0")]);
        apply(&hub_root, project.path()).unwrap();
        let dst = HubPaths::project_link_dst(project.path(), "demo");
        if lock_of(project.path()).entries[0].kind == LinkKind::Copy {
            // copy 模式无指向概念，此路径不适用
            return;
        }

        // 外部把链接重指到别的目录
        crate::link::remove_link(&dst).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        junction::create(elsewhere.path(), &dst).unwrap();
        assert!(
            status(&hub_root, project.path())
                .unwrap()
                .contains(&Drift::LinkMissing {
                    name: "demo".into()
                }),
            "重指的链接应按 LinkMissing 检出"
        );

        // 重跑 apply 自愈：链接指回物化目录，漂移清零
        apply(&hub_root, project.path()).unwrap();
        assert_eq!(status(&hub_root, project.path()).unwrap(), Vec::new());
    }
}
