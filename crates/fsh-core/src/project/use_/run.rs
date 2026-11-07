//! 使用的执行：按计划备好导出目录、建落点、写登记记录、顺手回收。
//! 任何一步失败都把本次已建落点删净——盘上不留没有记录的入口，登记簿里也不留没有入口的记录。

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::UsePlan;
use super::prepare::prepare_use;
use crate::git;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::project::{
    Anchor, CleanupReport, ProjectError, TargetOutcome, Targets, UseOutcome, anchors_of,
    lock_and_open, open_repo, rollback_links,
};

/// 一次使用：只有未归档 skill、各目标都无登记记录、全部落点空闲时才执行。
/// `agents` 中的重复项去重后登记；同一实际落点只建立一次入口。
/// dry_run = true 时预演：只读不落盘、不取写锁——别人正在写也能拿到动作清单。
pub fn use_skill(
    hub_root: &Path,
    key: &str,
    version: Option<&str>,
    agents: &[String],
    targets: Targets,
    dry_run: bool,
) -> Result<UseOutcome, ProjectError> {
    let anchors = anchors_of(&targets);
    // 目标集合为空是参数错误，先于 hub 装配检查报出
    if anchors.is_empty() {
        return Err(ProjectError::NoTarget);
    }
    if dry_run {
        let (repo, paths) = open_repo(hub_root)?;
        let plan = prepare_use(&repo, &paths, key, version, agents, anchors)?;
        return Ok(planned_outcome(key, plan));
    }

    // 这次使用跨导出、建链接、写登记簿、回收四次外部交互，用总耗时回答「慢在哪」
    let started = Instant::now();
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let UsePlan {
        source,
        version: planned_version,
        export_dir,
        mut targets,
    } = prepare_use(&repo, &paths, key, version, agents, anchors)?;

    // 固定版本：先确保导出目录就位（一致复用、不一致保留现场报错）。
    // 各目标共用一个版本，先到的那个落盘，其余看到目录已存在即跳过
    if let (Some(version), Some(dir)) = (&planned_version, &export_dir)
        && !dir.is_dir()
    {
        let tag = crate::naming::version_tag(key, version);
        git::materialize(&repo, &tag, &crate::naming::store_subpath(key), dir)?;
    }

    // 逐落点建入口；任一步失败即回滚本次已建的全部入口
    let mut created: Vec<(PathBuf, LinkKind)> = Vec::new();
    for target in &mut targets {
        for dest in &target.destinations {
            match link::create_dir_link(&source, Path::new(&dest.path)) {
                Ok(kind) => {
                    created.push((PathBuf::from(&dest.path), kind));
                    target.link = Some(kind);
                }
                Err(e) => {
                    // 建链接失败是本次使用的失败边界：记下哪个落点失败与原因，随后整批回滚
                    tracing::warn!(path = %dest.path, cause = %e, "落点建链接失败");
                    rollback_links(&created);
                    return Err(e.into());
                }
            }
        }
    }

    // 一次使用的全部落点必须同一种链接方式：混用没法在一条登记记录里用一个值表达
    let mut kinds = created.iter().map(|(_, kind)| *kind);
    if let Some(first) = kinds.next()
        && kinds.any(|kind| kind != first)
    {
        tracing::warn!(count = created.len(), "落点链接方式混用失败");
        let kinds = created
            .iter()
            .map(|(_, kind)| format!("{kind:?}"))
            .collect();
        let paths = created
            .iter()
            .map(|(path, _)| path.display().to_string())
            .collect();
        rollback_links(&created);
        return Err(ProjectError::MixedLinkKinds { kinds, paths });
    }
    for target in &targets {
        let Some(kind) = target.link else { continue };
        let count = target.destinations.len();
        if kind == LinkKind::Copy {
            // 档位决定落点后续怎么维护：copy 不与源同步，是链接降级，必须可查
            tracing::warn!(path = %source.display(), count = count, "落点降级");
        } else {
            // 链接方式选择是诊断细节：DEBUG 记一次选定，排查落点形态时对照
            tracing::debug!(path = %source.display(), count = count, "链接方式选定");
        }
    }

    // 登记簿：各目标一次写入，项目目标 upsert + 技能记录，全局目标进全局段（与 GC 引用来源同一份）；
    // 预检已把项目根规范成绝对路径，登记簿与落点因此共用同一身份。
    // 读不出登记簿同样回滚：否则留下没有记录的落点
    let mut registry = match hub::load_registry(&paths) {
        Ok(registry) => registry,
        Err(e) => {
            rollback_links(&created);
            return Err(e.into());
        }
    };
    for target in &targets {
        let Some(kind) = target.link else { continue };
        let record = hub::ProjectSkill {
            key: key.to_string(),
            version: planned_version.clone(),
            agents: target.agents.clone(),
            link: kind,
        };
        match &target.anchor {
            Anchor::Project(dir) => {
                registry.upsert(dir, hub::unix_now());
                // upsert 刚从身份键建过这条记录，取不到只可能是登记簿内部不一致
                let Some(entry) = registry.project_mut(dir) else {
                    rollback_links(&created);
                    return Err(ProjectError::InvalidProject {
                        path: dir.display().to_string(),
                    });
                };
                entry.skills.push(record);
            }
            Anchor::Global => registry.global_mut().skills.push(record),
        }
    }
    if let Err(e) = hub::save_registry(&paths, &registry, &token) {
        rollback_links(&created);
        return Err(e.into());
    }

    // 登记簿更新后即可回收无引用导出目录；回收失败不阻断使用，但必须留痕可查
    let cleanup = match crate::gc::sweep_after_use(&repo, &paths, &token) {
        Ok(report) => Some(CleanupReport {
            removed: report.items.len(),
            freed_bytes: report.freed_bytes,
        }),
        Err(e) => {
            tracing::warn!(cause = %e, "使用后导出目录回收失败");
            None
        }
    };

    // 使用完成是业务里程碑：逐目标各记一条，读日志即可重建「哪个目标铺了什么」
    let elapsed_ms = started.elapsed().as_millis() as u64;
    for target in &targets {
        target
            .anchor
            .log_milestone(key, target.destinations.len(), elapsed_ms, "使用完成");
    }
    Ok(UseOutcome {
        key: key.to_string(),
        version: planned_version,
        targets: targets
            .into_iter()
            .map(|target| TargetOutcome {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                link: target.link,
                destinations: target.destinations,
                actions: Vec::new(),
            })
            .collect(),
        cleanup,
        ..UseOutcome::default()
    })
}

/// 预演结果：各目标的身份、agent、计划落点与动作清单齐备；
/// 链接方式要真建才知道档位，预演因此不给
fn planned_outcome(key: &str, plan: UsePlan) -> UseOutcome {
    UseOutcome {
        dry_run: true,
        key: key.to_string(),
        version: plan.version,
        targets: plan
            .targets
            .into_iter()
            .map(|target| TargetOutcome {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                destinations: target.destinations,
                actions: target.actions,
                ..TargetOutcome::default()
            })
            .collect(),
        ..UseOutcome::default()
    }
}
