//! 项目使用：把未归档 skill 按版本铺进项目的 agent 技能目录，并整体撤回。
//!
//! 首次使用先做全部预检（skill 未归档、无登记、全部落点空闲），任何一项不通过
//! 都不动盘；执行中途失败回滚本次新增落点。撤回以整条技能记录为单位，先检查
//! 全部落点再删除，修改过的 copy 与被替换的落点不会被静默销毁。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
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
    /// hub 未初始化（无 .git）
    #[error("hub 未初始化: {0}")]
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

/// use 的结果：语义版本按需出现，实际落点逐个列出
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseReport {
    /// skill 唯一键
    pub key: String,
    /// 固定版本；缺省表示跟随最新
    pub version: Option<String>,
    /// 项目根绝对路径
    pub project: String,
    /// 本次登记的全部 agent
    pub agents: Vec<String>,
    /// 实际采用的链接方式（作用于全部落点）；预演时是占位值
    pub link: LinkKind,
    /// 实际建立的落点
    pub destinations: Vec<Destination>,
    /// 使用后顺手回收的导出目录统计；未执行或失败时为 None
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

/// unuse 的结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnuseReport {
    /// skill 唯一键
    pub key: String,
    /// 项目根绝对路径
    pub project: String,
    /// 实际删除的落点
    pub removed: Vec<String>,
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
        && !crate::skill::valid_semver(v)
    {
        return Err(ProjectError::InvalidSkillSpec {
            spec: spec.to_string(),
        });
    }
    Ok((key.to_string(), version.map(str::to_string)))
}

/// 首次项目使用：只有未归档 skill、无登记记录、全部落点空闲时才执行。
/// `agents` 中的重复项去重后登记；同一实际落点只建立一次入口。
pub fn use_skill(
    hub_root: &Path,
    key: &str,
    version: Option<&str>,
    agents: &[String],
    project_dir: &Path,
    dry_run: bool,
) -> Result<UseReport, ProjectError> {
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_use(&repo, &paths, key, version, agents, project_dir, dry_run)?;
    let mut actions = plan.actions.clone();
    if dry_run {
        // 预演只报告，不发生任何磁盘或登记簿变化
        debug_assert!(!actions.is_empty());
        return Ok(plan.report);
    }

    // 固定版本：先确保导出目录就位（一致复用、不一致保留现场报错）
    if let (Some(_), Some(export_dir)) = (&plan.version, &plan.export_dir)
        && !export_dir.is_dir()
    {
        let tag = crate::naming::version_tag(key, plan.version.as_deref().unwrap_or_default());
        git::materialize(&repo, &tag, &crate::naming::store_subpath(key), export_dir)?;
    }

    // 逐落点建链接；任一失败或档位不一致，回滚本次已建落点
    let mut created: Vec<(PathBuf, LinkKind)> = Vec::new();
    let mut kinds: Vec<LinkKind> = Vec::new();
    for dest in &plan.destinations {
        match link::create_dir_link(&plan.source, Path::new(&dest.path)) {
            Ok(kind) => {
                created.push((PathBuf::from(&dest.path), kind));
                kinds.push(kind);
            }
            Err(e) => {
                rollback_links(&created);
                return Err(e.into());
            }
        }
    }
    if let Some(first) = kinds.first()
        && kinds.iter().any(|k| k != first)
    {
        let paths: Vec<String> = created
            .iter()
            .map(|(p, _)| p.display().to_string())
            .collect();
        rollback_links(&created);
        return Err(ProjectError::MixedLinkKinds {
            kinds: kinds.iter().map(|k| format!("{k:?}")).collect(),
            paths,
        });
    }
    let kind = kinds.first().copied().unwrap_or(LinkKind::Copy);

    // 登记簿：项目 upsert + 技能记录（与 GC 引用来源同一份）；
    // 预检已把项目根规范成绝对路径，登记簿与落点因此共用同一身份
    let project_root = Path::new(&plan.project);
    let mut registry = hub::load_registry(&paths)?;
    registry.upsert(project_root, hub::unix_now());
    let record = registry
        .projects
        .iter_mut()
        .find(|p| path_identity(Path::new(&p.path)) == path_identity(project_root))
        .ok_or_else(|| ProjectError::InvalidProject {
            path: plan.project.clone(),
        })?;
    record.skills.push(hub::ProjectSkill {
        key: key.to_string(),
        version: plan.version.clone(),
        agents: plan.agents.clone(),
        link: kind,
    });
    hub::save_registry(&paths, &registry, &token)?;

    // 登记簿更新后即可回收无引用导出目录；回收失败不阻断使用，但必须留痕可查
    let cleanup = match crate::gc::sweep_after_use(&repo, &paths, &token) {
        Ok(report) => Some(CleanupReport {
            removed: report.items.len(),
            freed_bytes: report.freed_bytes,
        }),
        Err(e) => {
            tracing::warn!(cause = %e, "项目使用后的导出目录回收失败，可稍后跑 fsh gc");
            None
        }
    };

    actions.clear();
    Ok(UseReport {
        key: key.to_string(),
        version: plan.version,
        project: plan.project,
        agents: plan.agents,
        link: kind,
        destinations: plan.destinations,
        cleanup,
    })
}

/// use 预演：与实跑共用同一套预检，只返回动作清单，零落盘
pub fn use_dry(
    hub_root: &Path,
    key: &str,
    version: Option<&str>,
    agents: &[String],
    project_dir: &Path,
) -> Result<Vec<String>, ProjectError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_use(&repo, &paths, key, version, agents, project_dir, true)?;
    Ok(plan.actions)
}

/// use 的公共预检结果：实跑与预演由此同源
struct UsePlan {
    /// 技能源目录（工作副本或导出目录）
    source: PathBuf,
    /// 固定版本
    version: Option<String>,
    /// 固定版本的导出目录（跟随最新时为空）
    export_dir: Option<PathBuf>,
    /// 项目根绝对路径
    project: String,
    /// 去重后的 agent 键
    agents: Vec<String>,
    /// 去重后的落点
    destinations: Vec<Destination>,
    /// 预演动作清单
    actions: Vec<String>,
    /// 预演时给调用方的报告骨架
    report: UseReport,
}

/// 把 agent 键解析成实际落点：落点 = 项目根 + agent 技能目录 + 键末段，
/// 共用同一目录的 agent 合并成一条，保证一次使用只建一个入口
fn resolve_destinations(
    paths: &HubPaths,
    project: &Path,
    agent_keys: &[String],
    key: &str,
) -> Result<Vec<Destination>, ProjectError> {
    let skill_name = key.rsplit('/').next().unwrap_or(key);
    let mut grouped: BTreeMap<String, Destination> = BTreeMap::new();
    for agent_key in agent_keys {
        let agent = agents::resolve(paths, agent_key)?;
        let dest = project.join(&agent.skills_dir).join(skill_name);
        grouped
            .entry(path_identity(&dest))
            .or_insert_with(|| Destination {
                path: dest.display().to_string(),
                agents: Vec::new(),
            })
            .agents
            .push(agent_key.clone());
    }
    Ok(grouped.into_values().collect())
}

/// 共用预检：skill 未归档、版本存在、项目存在、无登记、落点全空闲
fn prepare_use(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    version: Option<&str>,
    agents: &[String],
    project_dir: &Path,
    dry_run: bool,
) -> Result<UsePlan, ProjectError> {
    let versions = git::tag_list(repo, key)?;
    let workdir = paths.skill_workdir(key);
    if !workdir.is_dir() {
        if versions.is_empty() {
            return Err(ProjectError::SkillNotFound { key: key.into() });
        }
        return Err(ProjectError::Archived { key: key.into() });
    }
    if let Some(v) = version
        && !versions.iter().any(|have| have == v)
    {
        return Err(ProjectError::VersionNotFound {
            key: key.into(),
            version: v.into(),
        });
    }
    let project = canonical_project(project_dir)?;

    let mut agent_keys: Vec<String> = Vec::new();
    for a in agents {
        if !agent_keys.iter().any(|e| e == a) {
            agent_keys.push(a.clone());
        }
    }
    if agent_keys.is_empty() {
        return Err(ProjectError::InvalidSkillSpec {
            spec: String::new(),
        });
    }

    // 登记簿闸门：该项目已有此技能记录时拒绝（升级 / 回滚先 unuse）
    let registry = hub::load_registry(paths)?;
    let already = registry
        .projects
        .iter()
        .find(|p| path_identity(Path::new(&p.path)) == path_identity(&project))
        .is_some_and(|p| p.skills.iter().any(|s| s.key == key));
    if already {
        return Err(ProjectError::AlreadyRegistered {
            key: key.into(),
            project: project.display().to_string(),
        });
    }

    let destinations = resolve_destinations(paths, &project, &agent_keys, key)?;

    // 全部落点必须空闲；父目录上有同名文件同样算占用
    for d in &destinations {
        if d.path.is_empty() || Path::new(&d.path).symlink_metadata().is_ok() {
            return Err(ProjectError::DestinationOccupied {
                path: d.path.clone(),
            });
        }
        ensure_parent_is_dir(Path::new(&d.path), &project)?;
    }

    let (source, export_dir, version_owned) = match version {
        Some(v) => {
            let dir = paths.exported(key, v);
            if dir.is_dir() {
                ensure_export_matches(repo, key, v, &dir)?;
            }
            (dir.clone(), Some(dir), Some(v.to_string()))
        }
        None => (workdir.clone(), None, None),
    };

    let mut actions = Vec::new();
    if let Some(dir) = export_dir.as_ref().filter(|d| !d.is_dir()) {
        actions.push(format!(
            "export {} -> {}",
            version_owned.as_deref().unwrap_or(""),
            dir.display()
        ));
    }
    for d in &destinations {
        actions.push(format!(
            "link {} -> {}（agents: {}）",
            source.display(),
            d.path,
            d.agents.join(", ")
        ));
    }
    actions.push(format!("register {} in {}", key, project.display()));
    if dry_run {
        actions.insert(0, "dry-run：以下动作不落盘".to_string());
    }

    let report = UseReport {
        key: key.to_string(),
        version: version_owned.clone(),
        project: project.display().to_string(),
        agents: agent_keys,
        // 预演不声称实际档位：固定填 copy 仅作占位，实跑报告里才是真实档位
        link: LinkKind::Copy,
        destinations: destinations.clone(),
        cleanup: None,
    };
    Ok(UsePlan {
        source,
        version: version_owned,
        export_dir,
        project: project.display().to_string(),
        agents: report.agents.clone(),
        destinations,
        actions,
        report,
    })
}

/// 整体撤回：该项目中此技能的全部 agent 落点。删除前检查全部落点，
/// 任一不合格则整个操作不动盘；`force` 只覆盖「copy 内容被修改」。
pub fn unuse(
    hub_root: &Path,
    key: &str,
    project_dir: &Path,
    force: bool,
    dry_run: bool,
) -> Result<UnuseReport, ProjectError> {
    let (token, _repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_unuse(&paths, key, project_dir, force)?;
    if dry_run {
        return Ok(plan.report);
    }
    for dest in &plan.destinations {
        link::remove_link(Path::new(&dest.path), plan.kind)?;
    }
    // 删除整条技能记录；项目没有其他技能时连项目记录一起删
    let mut registry = hub::load_registry(&paths)?;
    for p in registry.projects.iter_mut() {
        if path_identity(Path::new(&p.path)) == path_identity(Path::new(&plan.project)) {
            p.skills.retain(|s| s.key != key);
        }
    }
    registry.projects.retain(|p| !p.skills.is_empty());
    hub::save_registry(&paths, &registry, &token)?;
    Ok(plan.report)
}

/// unuse 预演：同一套检查，返回将删除的落点，零落盘
pub fn unuse_dry(
    hub_root: &Path,
    key: &str,
    project_dir: &Path,
    force: bool,
) -> Result<Vec<String>, ProjectError> {
    let (_token, _repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_unuse(&paths, key, project_dir, force)?;
    Ok(plan
        .destinations
        .iter()
        .map(|d| format!("remove {}（{:?}）", d.path, plan.kind))
        .collect())
}

/// unuse 的公共检查结果
struct UnusePlan {
    /// 项目根绝对路径
    project: String,
    /// 该技能记录登记的链接方式
    kind: LinkKind,
    /// 落点（与登记 agent 现算）
    destinations: Vec<Destination>,
    /// 结果报告
    report: UnuseReport,
}

/// 共用检查：项目与记录存在；全部落点形态与登记一致，copy 未被修改
fn prepare_unuse(
    paths: &HubPaths,
    key: &str,
    project_dir: &Path,
    force: bool,
) -> Result<UnusePlan, ProjectError> {
    let project = canonical_project(project_dir)?;
    let registry = hub::load_registry(paths)?;
    let record = registry
        .projects
        .iter()
        .find(|p| path_identity(Path::new(&p.path)) == path_identity(&project))
        .ok_or_else(|| ProjectError::NotRegistered {
            key: key.into(),
            project: project.display().to_string(),
        })?;
    let skill =
        record
            .skills
            .iter()
            .find(|s| s.key == key)
            .ok_or_else(|| ProjectError::NotRegistered {
                key: key.into(),
                project: project.display().to_string(),
            })?;

    let source = match &skill.version {
        Some(v) => paths.exported(key, v),
        None => paths.skill_workdir(key),
    };
    let destinations = resolve_destinations(paths, &project, &skill.agents, key)?;

    for d in &destinations {
        let path = Path::new(&d.path);
        // 落点已不存在 = 已撤回，幂等跳过
        let Ok(meta) = path.symlink_metadata() else {
            continue;
        };
        match skill.link {
            LinkKind::Copy => {
                if meta.file_type().is_symlink() || !meta.is_dir() {
                    return Err(ProjectError::LinkKindMismatch {
                        path: d.path.clone(),
                        expected: skill.link,
                        actual: describe_kind(&meta),
                    });
                }
                if !dirs_equal(&source, path)? && !force {
                    return Err(ProjectError::ModifiedCopy {
                        path: d.path.clone(),
                    });
                }
            }
            LinkKind::Symlink | LinkKind::Junction => {
                if !meta.file_type().is_symlink() {
                    return Err(ProjectError::LinkKindMismatch {
                        path: d.path.clone(),
                        expected: skill.link,
                        actual: describe_kind(&meta),
                    });
                }
                // 链接指向也必须仍是这次使用的源，换了靶子就不是本工具建的入口
                let target_ok = fs::canonicalize(path)
                    .ok()
                    .zip(fs::canonicalize(&source).ok())
                    .is_some_and(|(a, b)| path_identity(&a) == path_identity(&b));
                if !target_ok {
                    return Err(ProjectError::LinkKindMismatch {
                        path: d.path.clone(),
                        expected: skill.link,
                        actual: "链接目标已改变".to_string(),
                    });
                }
            }
        }
    }

    Ok(UnusePlan {
        project: project.display().to_string(),
        kind: skill.link,
        report: UnuseReport {
            key: key.to_string(),
            project: project.display().to_string(),
            removed: destinations.iter().map(|d| d.path.clone()).collect(),
        },
        destinations,
    })
}

/// 建链接 / 删链接前的统一入口：取全局写锁并确认 hub 已初始化
fn lock_and_open(
    hub_root: &Path,
) -> Result<(WriteToken, git2::Repository, HubPaths), ProjectError> {
    if !hub_root.join(".git").exists() {
        return Err(ProjectError::HubNotInitialized(
            hub_root.display().to_string(),
        ));
    }
    let paths = HubPaths::from_root(hub_root);
    let token = crate::lock::write_lock(&paths)?;
    let repo = git2::Repository::open(hub_root).map_err(git::GitError::from)?;
    Ok((token, repo, paths))
}

/// 倒序撤销已建落点：失败也继续删其余入口，保证不留下半套入口
fn rollback_links(created: &[(PathBuf, LinkKind)]) {
    for (path, kind) in created.iter().rev() {
        let _ = link::remove_link(path, *kind);
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

/// 路径身份：统一正斜杠、去掉 Windows 扩展前缀，Windows 下统一小写；
/// 登记簿身份、落点去重与落点形态核验共用这一个口径
fn path_identity(path: &Path) -> String {
    // 反斜杠归一成单斜杠；Windows canonicalize 的 `\?\` 前缀去掉，
    // 使同一目录无论是否带扩展前缀都得到同一身份
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s
        .strip_prefix("//?/")
        .or_else(|| s.strip_prefix("//./"))
        .unwrap_or(&s);
    let s = s.strip_prefix('/').unwrap_or(s);
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

/// 落点父目录链上最近的存在项必须是目录：否则建链接会在中途失败
fn ensure_parent_is_dir(dest: &Path, project: &Path) -> Result<(), ProjectError> {
    let mut cur = dest.parent();
    while let Some(dir) = cur {
        if dir.symlink_metadata().is_ok() {
            if dir.is_dir() {
                return Ok(());
            }
            return Err(ProjectError::DestinationOccupied {
                path: dir.display().to_string(),
            });
        }
        if path_identity(dir) == path_identity(project) {
            break;
        }
        cur = dir.parent();
    }
    Ok(())
}

/// 已有导出目录必须先与 tag 一致才复用；不一致保留现场并报错
fn ensure_export_matches(
    repo: &git2::Repository,
    key: &str,
    version: &str,
    dir: &Path,
) -> Result<(), ProjectError> {
    let tag = crate::naming::version_tag(key, version);
    let expected = crate::gc::hash_tag(repo, &tag, key);
    let actual = crate::gc::hash_dir_best_effort(dir);
    if expected == actual {
        Ok(())
    } else {
        Err(ProjectError::DirtyExport {
            path: dir.display().to_string(),
        })
    }
}

/// 落点实际形态的人读描述（报错用）
fn describe_kind(meta: &fs::Metadata) -> String {
    if meta.file_type().is_symlink() {
        "符号链接".to_string()
    } else if meta.is_dir() {
        "普通目录".to_string()
    } else {
        "普通文件".to_string()
    }
}

/// 两个目录内容是否逐文件一致：相对路径集合相同、同名文件字节相同。
/// 读不了任一侧时按不一致处理——宁可要求 --force，也不误删
fn dirs_equal(a: &Path, b: &Path) -> Result<bool, ProjectError> {
    let left = collect_relative_files(a)?;
    let right = collect_relative_files(b)?;
    if left != right {
        return Ok(false);
    }
    for rel in &left {
        let (x, y) = (a.join(rel), b.join(rel));
        if fs::read(&x)? != fs::read(&y)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// 递归收集目录内全部文件的相对路径（正斜杠，排序）
fn collect_relative_files(dir: &Path) -> Result<Vec<String>, ProjectError> {
    fn walk(base: &Path, cur: &Path, out: &mut Vec<String>) -> Result<(), ProjectError> {
        let mut entries: Vec<_> = fs::read_dir(cur)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            if p.is_dir() {
                walk(base, &p, out)?;
            } else {
                let rel = p
                    .strip_prefix(base)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
