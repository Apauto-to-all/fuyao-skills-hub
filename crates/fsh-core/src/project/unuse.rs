//! 撤回：把某技能在一次调用里的全部目标（可选的全局目标 + 零到多个项目目标）
//! 的落点撤走，并删掉对应的登记记录。给出 agent 名单时只撤这些 agent：从记录里
//! 摘掉它们，共用落点留到最后一个引用它的 agent 也撤走时才删。
//! 全部目标先一起检查，任一目标不合格则整个操作不动盘。

use std::path::Path;
use std::time::Instant;

use super::dest::{DestGates, DestVerdict, check_destination, resolve_global, resolve_project};
use super::skeleton::{Lead, Milestone, Session, lead, log_milestones, recorded_source};
use super::target::dedupe_anchors;
use super::{
    Anchor, Destination, ProjectError, Targets, UnuseOutcome, UnuseTarget, registered_skill,
};
use crate::agents;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

/// 撤回：`only` 给名单时只撤这些 agent 的落点并从记录里摘掉它们，
/// 不给时按整体撤回：撤该目标下此技能的全部落点并删掉那条登记记录。
/// 删除前检查全部目标，任一不合格则整个操作不动盘；`force` 只覆盖「copy 内容被修改」。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn unuse(
    hub_root: &Path,
    key: &str,
    targets: Targets,
    only: Option<&[String]>,
    force: bool,
    dry_run: bool,
) -> Result<UnuseOutcome, ProjectError> {
    // 相同前导（键校验、目标解析、无目标拒绝、预演分叉、取锁）三入口共用
    let Lead { anchors, session } = lead(hub_root, key, &targets, dry_run)?;
    let (token, paths, started) = match session {
        // 预演只读不落盘：动作清单照出
        Session::Dry { paths, .. } => {
            let plan = prepare_unuse(&paths, key, anchors, only, force)?;
            return Ok(planned_outcome(key, plan));
        }
        // 这次撤回跨删落点与写登记簿两次外部交互，用总耗时回答「慢在哪」
        Session::Write { token, paths, .. } => (token, paths, Instant::now()),
    };
    let plan = prepare_unuse(&paths, key, anchors, only, force)?;

    // 逐落点删；删到一半失败就停下，登记簿保持原样——已删的落点再跑一次会被当作
    // 「已撤回」跳过，所以重跑即可续完，不会留下登记簿与盘上不一致
    for target in &plan.targets {
        for dest in &target.destinations {
            if let Err(e) = link::remove_link(Path::new(&dest.path), target.kind) {
                tracing::warn!(path = %dest.path, cause = %e, "落点删除失败");
                return Err(e.into());
            }
        }
    }

    let mut registry = hub::load_registry(&paths)?;
    for target in &plan.targets {
        let skills = match &target.anchor {
            Anchor::Project(dir) => registry.project_mut(dir).map(|record| &mut record.skills),
            Anchor::Global => Some(&mut registry.global_mut().skills),
        };
        let Some(skills) = skills else { continue };
        // 剩下的 agent 为空 = 这条记录整体撤回；否则只把名单里的名字摘掉
        if target.remaining.is_empty() {
            skills.retain(|s| s.key != key);
        } else {
            for skill in skills.iter_mut().filter(|s| s.key == key) {
                skill.agents = target.remaining.clone();
            }
        }
    }
    // 项目没有其他技能时连项目记录一起删；全局段由 is_empty 决定是否写进文件
    registry
        .projects
        .retain(|_, record| !record.skills.is_empty());
    hub::save_registry(&paths, &registry, &token)?;

    // 撤回完成是业务里程碑：逐目标各记一条，读日志即可重建「撤了哪几处」
    log_milestones(
        key,
        started,
        plan.targets.iter().map(|target| Milestone {
            anchor: &target.anchor,
            version: target.version.as_deref(),
            count: target.destinations.len(),
            done: "解除使用完成",
            skipped: None,
        }),
    );
    Ok(UnuseOutcome {
        key: key.to_string(),
        targets: plan
            .targets
            .into_iter()
            .map(|target| UnuseTarget {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                link: Some(target.kind),
                removed: target.destinations.into_iter().map(|d| d.path).collect(),
                actions: Vec::new(),
            })
            .collect(),
        ..UnuseOutcome::default()
    })
}

/// 预演结果：各目标的身份、将删除的落点与动作清单
fn planned_outcome(key: &str, plan: UnusePlan) -> UnuseOutcome {
    UnuseOutcome {
        dry_run: true,
        key: key.to_string(),
        targets: plan
            .targets
            .into_iter()
            .map(|target| UnuseTarget {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                actions: target.actions,
                ..UnuseTarget::default()
            })
            .collect(),
    }
}

/// unuse 的公共检查结果
struct UnusePlan {
    /// 逐目标的检查结果，全局目标在最前
    targets: Vec<UnuseTargetPlan>,
}

/// 一个目标的检查结果
struct UnuseTargetPlan {
    /// 目标身份
    anchor: Anchor,
    /// 本次撤走的 agent（报告的 agents 列的就是这批）
    agents: Vec<String>,
    /// 记录里剩下的 agent；空表示整条记录撤回
    remaining: Vec<String>,
    /// 该技能记录登记的版本（跟随最新时为空）
    version: Option<String>,
    /// 该技能记录登记的链接方式
    kind: LinkKind,
    /// 真正要删的落点（与登记 agent 现算）
    destinations: Vec<Destination>,
    /// 将执行的动作清单
    actions: Vec<String>,
}

/// 共用检查：各目标的记录都在；全部落点形态与登记一致，copy 未被修改。
/// 落点是整条记录解析出来的，因此保留方的 agent 也一并被解析——配置坏的保留方会被点名，
/// 共用落点也就有了判断依据。
fn prepare_unuse(
    paths: &HubPaths,
    key: &str,
    anchors: Vec<Anchor>,
    only: Option<&[String]>,
    force: bool,
) -> Result<UnusePlan, ProjectError> {
    let registry = hub::load_registry(paths)?;
    // agent 清单只读一次，逐目标复用
    let known = agents::list(paths)?;
    let mut targets = Vec::new();
    for anchor in dedupe_anchors(anchors)? {
        let skill = registered_skill(&registry, &anchor, key)?;
        let withdrawn = withdrawn_agents(&skill.agents, key, only)?;
        // 记录推导出的源目录与目标版本同一条规则：固定版本链导出目录，跟随最新链工作副本
        let source = recorded_source(paths, key, skill.version.as_deref());
        let resolved = match &anchor {
            Anchor::Project(dir) => resolve_project(&known, dir, &skill.agents, key)?,
            Anchor::Global => resolve_global(&known, &skill.agents, key)?,
        };
        // 共用落点：只有引用它的 agent 全在撤回名单里时才删，否则留给还在用的那一方
        let (doomed, kept): (Vec<Destination>, Vec<Destination>) = resolved
            .into_iter()
            .partition(|dest| dest.agents.iter().all(|agent| withdrawn.contains(agent)));
        // 保留是撤回里唯一的删除决策点：记下哪处落点被留下、还有谁在用，日志里才分得清
        for dest in &kept {
            tracing::debug!(path = %dest.path, agents = %dest.agents.join(","), "共用落点保留");
        }
        let destinations = check_destinations(&doomed, skill.link, &source, force)?;
        targets.push(UnuseTargetPlan {
            anchor,
            remaining: skill
                .agents
                .iter()
                .filter(|agent| !withdrawn.contains(agent))
                .cloned()
                .collect(),
            agents: withdrawn,
            version: skill.version.clone(),
            kind: skill.link,
            actions: destinations
                .iter()
                .map(|d| format!("remove {}（{:?}）", d.path, skill.link))
                .collect(),
            destinations,
        });
    }
    Ok(UnusePlan { targets })
}

/// 本次撤走的 agent：不给名单就是整条记录的全部 agent；
/// 给了名单就必须是记录里已有的，否则点名该 agent 与已登记清单。
/// 空名单是合法的空撤回：落点与登记都不动，记录原样留着。
fn withdrawn_agents(
    registered: &[String],
    key: &str,
    only: Option<&[String]>,
) -> Result<Vec<String>, ProjectError> {
    let Some(only) = only else {
        return Ok(registered.to_vec());
    };
    for agent in only {
        if !registered.contains(agent) {
            return Err(ProjectError::AgentNotRegistered {
                key: key.to_string(),
                agent: agent.clone(),
                known: registered.join(", "),
            });
        }
    }
    Ok(registered
        .iter()
        .filter(|agent| only.contains(agent))
        .cloned()
        .collect())
}

/// 逐落点过三闸，返回真正要删的落点——已缺失的落点视为已撤回，直接剔掉。
/// 撤回要动每一个落点，拷贝内容闸因此全开
fn check_destinations(
    destinations: &[Destination],
    kind: LinkKind,
    source: &Path,
    force: bool,
) -> Result<Vec<Destination>, ProjectError> {
    let gates = DestGates {
        kind,
        source,
        guard_copy: true,
        force,
    };
    let mut live = Vec::new();
    for d in destinations {
        match check_destination(d, &gates)? {
            // 幂等命中是撤回里唯一的决策点：记下被跳过的落点，核对实际删除范围
            DestVerdict::Missing => tracing::warn!(path = %d.path, "落点已撤回跳过"),
            DestVerdict::Live => live.push(d.clone()),
        }
    }
    Ok(live)
}
