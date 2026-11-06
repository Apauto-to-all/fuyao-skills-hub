//! 项目使用：首次把某未归档技能按版本铺给项目的若干 agent，并登记一条技能记录。
//! 预检与实跑同源——预演只报告动作，实跑按同一份计划落盘。

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::dest::{ensure_export_matches, ensure_parent_is_dir, resolve_destinations};
use super::{CleanupReport, Destination, ProjectError, UseReport};
use super::{canonical_project, lock_and_open, rollback_links};
use crate::git;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

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
    // 这次使用跨导出、建链接、写登记簿、回收四次外部交互，用总耗时回答「慢在哪」
    let started = Instant::now();
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
                // 建链接失败是本次使用的失败边界：记下哪个落点失败与原因，随后整批回滚
                tracing::warn!(path = %dest.path, cause = %e, "落点建链接失败");
                rollback_links(&created);
                return Err(e.into());
            }
        }
    }
    if let Some(first) = kinds.first()
        && kinds.iter().any(|k| k != first)
    {
        // 档位混用无法在登记簿里用一个值表达，属于已回滚的失败，必须留痕
        tracing::warn!(count = kinds.len(), "落点链接方式混用失败");
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
    // 档位决定落点后续怎么维护：copy 不与源同步，是链接降级，必须可查
    if kind == LinkKind::Copy {
        tracing::warn!(
            path = %plan.source.display(),
            count = plan.destinations.len(),
            "落点降级"
        );
    } else {
        // 链接方式选择是诊断细节：DEBUG 记一次选定，排查落点形态时对照
        tracing::debug!(
            path = %plan.source.display(),
            count = plan.destinations.len(),
            "链接方式选定"
        );
    }

    // 登记簿：项目 upsert + 技能记录（与 GC 引用来源同一份）；
    // 预检已把项目根规范成绝对路径，登记簿与落点因此共用同一身份
    let project_root = Path::new(&plan.project);
    let mut registry = hub::load_registry(&paths)?;
    registry.upsert(project_root, hub::unix_now());
    let record = registry
        .projects
        .iter_mut()
        .find(|p| p.matches(project_root))
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
            tracing::warn!(cause = %e, "项目使用后导出目录回收失败");
            None
        }
    };

    actions.clear();
    // 使用完成是业务里程碑：key、项目、落点数与总耗时足以重建这次操作
    tracing::info!(
        key = %key,
        project = %plan.project,
        count = plan.destinations.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "项目使用完成"
    );
    Ok(UseReport {
        key: key.to_string(),
        version: plan.version,
        project: crate::identity::display_path(std::path::Path::new(&plan.project)),
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
        .find(|p| p.matches(&project))
        .is_some_and(|p| p.skills.iter().any(|s| s.key == key));
    if already {
        return Err(ProjectError::AlreadyRegistered {
            key: key.into(),
            project: crate::identity::display_path(&project),
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
        project: crate::identity::display_path(&project),
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
        project: crate::identity::display_path(&project),
        agents: report.agents.clone(),
        destinations,
        actions,
        report,
    })
}
