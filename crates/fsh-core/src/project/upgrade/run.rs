//! 版本切换的执行：按计划备好导出目录、逐落点换靶、写登记记录、删占位、顺手回收。
//! 任何一步失败都把占位改回原名并删掉本次新建的入口——盘上回到旧状态，登记簿不动。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::prepare::prepare_upgrade;
use super::{UpgradePlan, placeholder_of};
use crate::git;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::project::{
    Anchor, CleanupReport, ProjectError, Targets, UpgradeOutcome, UpgradeTarget, anchors_of,
    lock_and_open, open_repo, rollback_links,
};

/// 一次版本切换：只改已有登记记录，目标里没有该技能的记录即报错不动盘。
/// dry_run = true 时预演：只读不落盘、不取写锁——别人正在写也能拿到动作清单。
pub fn upgrade(
    hub_root: &Path,
    key: &str,
    version: Option<&str>,
    targets: Targets,
    force: bool,
    dry_run: bool,
) -> Result<UpgradeOutcome, ProjectError> {
    // 键形状先判：CLI 之外的入口（界面）同样过这一道，规则不靠前端复写
    crate::key::validate(key)?;
    let anchors = anchors_of(&targets);
    // 目标集合为空是参数错误，先于 hub 装配检查报出
    if anchors.is_empty() {
        return Err(ProjectError::NoTarget);
    }
    if dry_run {
        let (repo, paths) = open_repo(hub_root)?;
        let plan = prepare_upgrade(&repo, &paths, key, version, anchors, force)?;
        return Ok(planned_outcome(key, plan));
    }

    // 这次版本切换跨导出、换靶、写登记簿、回收四次外部交互，用总耗时回答「慢在哪」
    let started = Instant::now();
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let UpgradePlan {
        to_version,
        source,
        export_dir,
        mut targets,
    } = prepare_upgrade(&repo, &paths, key, version, anchors, force)?;

    // 全目标都已在目标版本：版本切换没有盘动作，导出、换靶、写登记簿与回收一律不做
    let switching = targets.iter().any(|target| !target.idempotent);

    // 固定版本：先确保导出目录就位；换靶因此不会因为源缺失删掉旧入口
    if switching
        && let (Some(version), Some(dir)) = (&to_version, &export_dir)
        && !dir.is_dir()
    {
        let tag = crate::naming::version_tag(key, version);
        git::materialize(&repo, &tag, &crate::naming::store_subpath(key), dir)?;
    }

    // 换靶：旧入口先改名占位，原路径建新入口。任一步失败即整批还原
    let mut renamed: Vec<Placeholder> = Vec::new();
    let mut created: Vec<(PathBuf, LinkKind)> = Vec::new();
    for target in &mut targets {
        if target.idempotent {
            continue;
        }
        let mut swapped: Vec<(String, LinkKind)> = Vec::new();
        for dest in &target.destinations {
            let placeholder = placeholder_of(Path::new(&dest.path));
            if let Err(e) = fs::rename(Path::new(&dest.path), &placeholder) {
                // 换靶失败是本次版本切换的失败边界：记下哪个旧入口改不动，随后整批还原
                tracing::warn!(path = %dest.path, cause = %e, "旧入口改名占位失败");
                restore_swap(&renamed, &created);
                return Err(e.into());
            }
            renamed.push(Placeholder {
                original: PathBuf::from(&dest.path),
                path: placeholder,
                kind: target.kind,
            });
            match link::create_dir_link(&source, Path::new(&dest.path)) {
                Ok(kind) => {
                    created.push((PathBuf::from(&dest.path), kind));
                    swapped.push((dest.path.clone(), kind));
                    target.link = Some(kind);
                }
                Err(e) => {
                    tracing::warn!(path = %dest.path, cause = %e, "换靶落点建链接失败");
                    restore_swap(&renamed, &created);
                    return Err(e.into());
                }
            }
        }
        // 一条登记的全部落点必须同一种链接方式：混用没法在记录里用一个值表达
        if let Some(first) = swapped.first()
            && swapped.iter().any(|(_, kind)| *kind != first.1)
        {
            tracing::warn!(key = %key, count = swapped.len(), "记录内落点链接方式混用失败");
            let kinds = swapped
                .iter()
                .map(|(_, kind)| format!("{kind:?}"))
                .collect();
            let paths = swapped.iter().map(|(path, _)| path.clone()).collect();
            restore_swap(&renamed, &created);
            return Err(ProjectError::MixedLinkKinds { kinds, paths });
        }
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

    if switching {
        // 登记簿：换成的版本与实际档位一次写入；写不进去就整批还原，同一次操作不留半套
        let mut registry = match hub::load_registry(&paths) {
            Ok(registry) => registry,
            Err(e) => {
                restore_swap(&renamed, &created);
                return Err(e.into());
            }
        };
        for target in &targets {
            let Some(kind) = target.link else { continue };
            retarget(&mut registry, &target.anchor, key, to_version.clone(), kind);
        }
        if let Err(e) = hub::save_registry(&paths, &registry, &token) {
            restore_swap(&renamed, &created);
            return Err(e.into());
        }

        // 全部成功才删占位：删不掉只在原地留痕，已生效的入口不再回退
        for p in &renamed {
            if let Err(e) = link::remove_link(&p.path, p.kind) {
                tracing::warn!(path = %p.path.display(), cause = %e, "占位删除失败");
            }
        }
    }

    // 登记簿更新后即可回收无引用导出目录（换出来的旧版本目录随之清掉）；
    // 回收失败只留痕；一条记录都没换时整批跳过，幂等命中不留盘动作
    let cleanup = if switching {
        match crate::gc::sweep_after_use(&repo, &paths, &token) {
            Ok(report) => Some(CleanupReport {
                removed: report.items.len(),
                freed_bytes: report.freed_bytes,
            }),
            Err(e) => {
                tracing::warn!(cause = %e, "版本切换后导出目录回收失败");
                None
            }
        }
    } else {
        None
    };

    // 版本切换完成是业务里程碑：逐目标各记一条，读日志即可重建「哪条记录切到了哪一版」
    let elapsed_ms = started.elapsed().as_millis() as u64;
    for target in &targets {
        let count = target.destinations.len();
        if target.idempotent {
            // 幂等命中是本次唯一的跳过决策：没有盘动作，日志里要说清为什么什么都没变
            target.anchor.log_skip(
                key,
                to_version.as_deref(),
                count,
                elapsed_ms,
                "已处于目标版本跳过",
            );
            continue;
        }
        target.anchor.log_milestone(
            key,
            to_version.as_deref(),
            count,
            elapsed_ms,
            "版本切换完成",
        );
    }
    Ok(UpgradeOutcome {
        key: key.to_string(),
        version: to_version,
        targets: targets
            .into_iter()
            .map(|target| UpgradeTarget {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                from: target.from_version,
                idempotent: target.idempotent,
                // 幂等命中的目标没有换靶，报出的就是登记里那个未变的值
                link: target.link.or(if target.idempotent {
                    Some(target.kind)
                } else {
                    None
                }),
                destinations: target.destinations,
                actions: Vec::new(),
                note: target.note,
            })
            .collect(),
        cleanup,
        ..UpgradeOutcome::default()
    })
}

/// 预演结果：各目标的身份、切换前版本、幂等标记与动作清单齐备；
/// 链接方式要真建才知道档位，预演因此不给
fn planned_outcome(key: &str, plan: UpgradePlan) -> UpgradeOutcome {
    UpgradeOutcome {
        dry_run: true,
        key: key.to_string(),
        version: plan.to_version,
        targets: plan
            .targets
            .into_iter()
            .map(|target| UpgradeTarget {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                from: target.from_version,
                idempotent: target.idempotent,
                actions: target.actions,
                note: target.note,
                ..UpgradeTarget::default()
            })
            .collect(),
        ..UpgradeOutcome::default()
    }
}

/// 一次换靶留下的占位：旧入口原名、改名后的占位名与登记旧档位（占位按旧档位删）
struct Placeholder {
    /// 旧入口的原名
    original: PathBuf,
    /// 旧入口改名后的占位名
    path: PathBuf,
    /// 登记的旧档位
    kind: LinkKind,
}

/// 换靶回滚：先删本次新建的入口（原名位置得先空出来），再把占位改回原名
fn restore_swap(renamed: &[Placeholder], created: &[(PathBuf, LinkKind)]) {
    rollback_links(created);
    for p in renamed.iter().rev() {
        if let Err(e) = fs::rename(&p.path, &p.original) {
            tracing::warn!(path = %p.path.display(), cause = %e, "回滚占位改名失败");
        }
    }
}

/// 把某目标里该技能记录的版本与链接方式换掉，agent 名单不变。
/// 项目目标顺手刷新最后操作时间；记录在预检里已确认存在
fn retarget(
    registry: &mut hub::ProjectRegistry,
    anchor: &Anchor,
    key: &str,
    version: Option<String>,
    kind: LinkKind,
) {
    let skills = match anchor {
        Anchor::Project(dir) => {
            registry.upsert(dir, hub::unix_now());
            registry.project_mut(dir).map(|record| &mut record.skills)
        }
        Anchor::Global => Some(&mut registry.global_mut().skills),
    };
    let Some(skills) = skills else { return };
    for skill in skills.iter_mut().filter(|s| s.key == key) {
        skill.version = version.clone();
        skill.link = kind;
    }
}
