//! 项目撤回：把某技能在某项目的全部 agent 落点整体撤回，并删掉登记记录。
//! 不提供按 agent 部分撤回；先检查全部落点，任一不合格则整个操作不动盘。

use std::fs;
use std::path::Path;
use std::time::Instant;

use super::dest::{describe_kind, dirs_equal, resolve_destinations};
use super::{
    Destination, ProjectError, UnuseReport, canonical_project, lock_and_open, path_identity,
};
use crate::hub;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

/// 整体撤回：该项目中此技能的全部 agent 落点。删除前检查全部落点，
/// 任一不合格则整个操作不动盘；`force` 只覆盖「copy 内容被修改」。
pub fn unuse(
    hub_root: &Path,
    key: &str,
    project_dir: &Path,
    force: bool,
    dry_run: bool,
) -> Result<UnuseReport, ProjectError> {
    // 这次撤回跨删落点与写登记簿两次外部交互，用总耗时回答「慢在哪」
    let started = Instant::now();
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
    // 撤回完成是业务里程碑：key、项目、落点数与总耗时足以重建这次操作
    tracing::info!(
        key = %key,
        project = %plan.project,
        count = plan.report.removed.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "项目解除使用完成"
    );
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
            // 幂等命中是撤回里唯一的决策点：记下被跳过的落点，核对实际删除范围
            tracing::warn!(path = %d.path, "落点已撤回跳过");
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
