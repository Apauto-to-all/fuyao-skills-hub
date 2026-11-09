//! 版本切换的预检：逐目标找到登记记录、核对落点三闸、算出新源与动作清单。
//! 任一目标不合格即整体报错，实跑因此不会动盘；预演与实跑共用这一份计划。

use std::path::Path;

use super::{UpgradePlan, UpgradeTargetPlan, placeholder_of};
use crate::agents;
use crate::git;
use crate::hub;
use crate::link::LinkKind;
use crate::paths::HubPaths;
use crate::project::dest::{
    DestGates, DestVerdict, check_destination, ensure_export_matches, resolve_global,
    resolve_project,
};
use crate::project::target::dedupe_anchors;
use crate::project::{Anchor, Destination, ProjectError, registered_skill};
use crate::skill::skill_status;

/// 共用预检：技能侧一次（存在性、归档、版本、导出目录），
/// 再逐目标核对登记记录、落点三闸与占位残留。
pub(super) fn prepare_upgrade(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
    version: Option<&str>,
    anchors: Vec<Anchor>,
    force: bool,
) -> Result<UpgradePlan, ProjectError> {
    // 三态与版本清单由技能状态模块一次取全，这里的判定只剩「要什么」
    let status = skill_status(repo, paths, key)?;
    status.require_active()?;
    if let Some(v) = version {
        status.require_version(v)?;
    }
    let workdir = paths.skill_workdir(key);

    // 新源：固定版本先确保导出目录就位（一致复用、不一致保留现场报错），跟随最新直接链工作副本
    let (source, export_dir, to_version) = match version {
        Some(v) => {
            let dir = paths.exported(key, v);
            if dir.is_dir() {
                ensure_export_matches(repo, key, v, &dir)?;
            }
            (dir.clone(), Some(dir), Some(v.to_string()))
        }
        None => (workdir.clone(), None, None),
    };

    // 登记簿与 agent 清单各读一次，逐目标复用
    let registry = hub::load_registry(paths)?;
    let known = agents::list(paths)?;
    let mut targets: Vec<UpgradeTargetPlan> = Vec::new();
    for anchor in dedupe_anchors(anchors)? {
        let skill = registered_skill(&registry, &anchor, key)?;
        let from_version = skill.version.clone();
        // 记录推导出的旧源：固定版本链到导出目录，跟随最新链工作副本
        let old_source = match &from_version {
            Some(v) => paths.exported(key, v),
            None => workdir.clone(),
        };
        // 幂等判定在形态闸之后、拷贝闸之前：已在目标版本时不动盘，只核对形态与靶子
        let idempotent = from_version == to_version;
        let target_name = anchor.label();
        let destinations = match &anchor {
            Anchor::Project(dir) => resolve_project(&known, dir, &skill.agents, key)?,
            Anchor::Global => resolve_global(&known, &skill.agents, key)?,
        };
        let gates = DestGates {
            kind: skill.link,
            source: &old_source,
            guard_copy: !idempotent,
            force,
        };
        for dest in &destinations {
            if matches!(check_destination(dest, &gates)?, DestVerdict::Missing) {
                // 落点缺失是版本切换的失败边界：点名目标、落点与 agent，随后整次不动盘
                tracing::warn!(
                    key = %key,
                    path = %dest.path,
                    agents = %dest.agents.join(","),
                    "落点缺失失败"
                );
                return Err(ProjectError::DestinationMissing {
                    key: key.into(),
                    target: target_name.clone(),
                    path: dest.path.clone(),
                    agents: dest.agents.join(", "),
                });
            }
            // 占位只用于目录入口：名字被占用即上次中断的残骸，只点名、绝不代删
            if !idempotent {
                let placeholder = placeholder_of(Path::new(&dest.path));
                if placeholder.symlink_metadata().is_ok() {
                    // 占位残留是版本切换的失败边界：不自动清理，只在盘上点名
                    tracing::warn!(
                        key = %key,
                        path = %placeholder.display(),
                        "占位残留失败"
                    );
                    return Err(ProjectError::PlaceholderResidue {
                        path: placeholder.display().to_string(),
                    });
                }
            }
        }
        let actions = planned_actions(
            key,
            &Swap {
                idempotent,
                target_name: &target_name,
                kind: skill.link,
                from: from_version.as_deref(),
                to: to_version.as_deref(),
                source: &source,
                destinations: &destinations,
            },
        );
        let note = copy_note(skill.link, to_version.as_deref());
        targets.push(UpgradeTargetPlan {
            anchor,
            agents: skill.agents.clone(),
            from_version,
            kind: skill.link,
            idempotent,
            destinations,
            actions,
            note,
            link: None,
        });
    }
    if targets.is_empty() {
        return Err(ProjectError::NoTarget);
    }
    if let Some(dir) = export_dir.as_ref().filter(|d| !d.is_dir()) {
        let line = format!(
            "export {} -> {}",
            to_version.as_deref().unwrap_or(""),
            dir.display()
        );
        for target in &mut targets {
            target.actions.insert(0, line.clone());
        }
    }

    Ok(UpgradePlan {
        to_version,
        source,
        export_dir,
        targets,
    })
}

/// 版本人读名：固定版本写 `@<版本>`，跟随最新写「跟最新」
fn version_label(version: Option<&str>) -> String {
    match version {
        Some(v) => format!("@{v}"),
        None => "跟最新".to_string(),
    }
}

/// 幂等命中的提醒：记录已跟最新且落点是拷贝档时，盘上还是旧内容——拷贝是快照，不跟随源
fn copy_note(kind: LinkKind, to_version: Option<&str>) -> Option<String> {
    (kind == LinkKind::Copy && to_version.is_none())
        .then(|| "拷贝档不跟随源，要刷新就重新铺（unuse 后 use）".to_string())
}

/// 一个目标换靶前后的状态：动作清单按它拼出人读行
struct Swap<'a> {
    /// 该目标已处于目标版本
    idempotent: bool,
    /// 目标人读名（项目路径或「全局」）
    target_name: &'a str,
    /// 登记的链接方式
    kind: LinkKind,
    /// 切换前的登记版本
    from: Option<&'a str>,
    /// 换到的版本
    to: Option<&'a str>,
    /// 新入口的源目录
    source: &'a Path,
    /// 记录解析出的落点
    destinations: &'a [Destination],
}

/// 一个目标的动作清单：旧版本 → 新版本、逐落点换靶三步、登记变更。
/// 已在目标版本的目标没有盘动作，清单写明为什么什么都没做
fn planned_actions(key: &str, swap: &Swap<'_>) -> Vec<String> {
    let to_label = version_label(swap.to);
    if swap.idempotent {
        return vec![format!("{key} 已处于目标版本（{to_label}），无盘动作")];
    }
    let mut actions = vec![format!("{} -> {}", version_label(swap.from), to_label)];
    for dest in swap.destinations {
        let placeholder = placeholder_of(Path::new(&dest.path));
        actions.push(format!(
            "rename {} -> {}（{:?}）",
            dest.path,
            placeholder.display(),
            swap.kind
        ));
        actions.push(format!(
            "link {} -> {}（agents: {}）",
            swap.source.display(),
            dest.path,
            dest.agents.join(", ")
        ));
        actions.push(format!("remove {}", placeholder.display()));
    }
    actions.push(format!(
        "register {key}（{to_label}）in {}",
        swap.target_name
    ));
    actions
}
