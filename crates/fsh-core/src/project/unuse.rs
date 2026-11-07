//! 项目撤回：把某技能在某项目的全部 agent 落点整体撤回，并删掉登记记录。
//! 不提供按 agent 部分撤回；先检查全部落点，任一不合格则整个操作不动盘。

use std::fs;
use std::path::Path;
use std::time::Instant;

use super::dest::{describe_kind, dirs_equal, resolve_destinations};
use super::{Destination, ProjectError, UnuseOutcome, canonical_project, lock_and_open, open_repo};
use crate::hub;
use crate::identity::path_identity;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

/// 整体撤回：该项目中此技能的全部 agent 落点。删除前检查全部落点，
/// 任一不合格则整个操作不动盘；`force` 只覆盖「copy 内容被修改」。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn unuse(
    hub_root: &Path,
    key: &str,
    project_dir: &Path,
    force: bool,
    dry_run: bool,
) -> Result<UnuseOutcome, ProjectError> {
    if dry_run {
        let (_repo, paths) = open_repo(hub_root)?;
        let plan = prepare_unuse(&paths, key, project_dir, force)?;
        return Ok(UnuseOutcome {
            dry_run: true,
            actions: plan.actions,
            ..UnuseOutcome::default()
        });
    }
    // 这次撤回跨删落点与写登记簿两次外部交互，用总耗时回答「慢在哪」
    let started = Instant::now();
    let (token, _repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_unuse(&paths, key, project_dir, force)?;
    for dest in &plan.destinations {
        link::remove_link(Path::new(&dest.path), plan.kind)?;
    }
    // 删除整条技能记录；项目没有其他技能时连项目记录一起删
    let mut registry = hub::load_registry(&paths)?;
    if let Some(record) = registry.project_mut(Path::new(&plan.project)) {
        record.skills.retain(|s| s.key != key);
    }
    registry
        .projects
        .retain(|_, record| !record.skills.is_empty());
    hub::save_registry(&paths, &registry, &token)?;
    // 撤回完成是业务里程碑：key、项目、落点数与总耗时足以重建这次操作
    tracing::info!(
        key = %key,
        project = %plan.project,
        count = plan.removed.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "项目解除使用完成"
    );
    Ok(UnuseOutcome {
        key: key.to_string(),
        project: plan.project,
        removed: plan.removed,
        ..UnuseOutcome::default()
    })
}

/// unuse 的公共检查结果
struct UnusePlan {
    /// 项目根绝对路径
    project: String,
    /// 该技能记录登记的链接方式
    kind: LinkKind,
    /// 落点（与登记 agent 现算）
    destinations: Vec<Destination>,
    /// 将删除的落点路径
    removed: Vec<String>,
    /// 将执行的动作清单
    actions: Vec<String>,
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
        .project(&project)
        .ok_or_else(|| ProjectError::NotRegistered {
            key: key.into(),
            project: crate::identity::display_path(&project),
        })?;
    let skill =
        record
            .skills
            .iter()
            .find(|s| s.key == key)
            .ok_or_else(|| ProjectError::NotRegistered {
                key: key.into(),
                project: crate::identity::display_path(&project),
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

    let actions = destinations
        .iter()
        .map(|d| format!("remove {}（{:?}）", d.path, skill.link))
        .collect();
    Ok(UnusePlan {
        project: crate::identity::display_path(&project),
        kind: skill.link,
        removed: destinations.iter().map(|d| d.path.clone()).collect(),
        destinations,
        actions,
    })
}
