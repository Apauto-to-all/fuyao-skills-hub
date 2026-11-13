//! 撤回：把某技能在一次调用里的全部目标（可选的全局目标 + 零到多个项目目标）
//! 的落点撤走，并从对应的登记记录里摘掉点名的 agent；记录里没有别的 agent 时
//! 连记录一起删，名单点满整条记录即整条撤回。
//! 全部目标先一起检查，任一目标不合格则整个操作不动盘。

use std::path::Path;

use super::dest::{
    Dest, DestGates, DestVerdict, check_destination, resolve_global, resolve_project,
};
use super::ledger::{commit_registry, usage_message};
use super::target::{anchors_or_reject, dedupe_anchors};
use super::{Anchor, ProjectError, Targets, UnuseOutcome, UnuseTarget, registered_skill};
use crate::agents;
use crate::commit::Verb;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep};
use crate::skill::skill_occupancy;
use crate::write::{self, Applied, OnFailure};

/// 撤回：从每条登记记录里摘掉 `only` 点名的 agent，记录里没有别的 agent 时
/// 连记录一起删（名单点满整条记录即整条撤回）；落点留到引用它的 agent 全在
/// 本次名单里时才删，共用落点因此留给还在用的那一方。
/// 删除前检查全部目标，任一不合格则整个操作不动盘；`force` 只覆盖「copy 内容被修改」。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn unuse(
    hub_root: &Path,
    key: &str,
    targets: Targets,
    only: &[String],
    force: bool,
    dry_run: bool,
) -> Result<UnuseOutcome, ProjectError> {
    // 入参判定先于装配：键形状与目标集合都不依赖 hub
    crate::key::validate(key)?;
    let anchors = anchors_or_reject(&targets)?;
    write::run(
        hub_root,
        dry_run,
        // 删除不回滚：删到一半失败时已删的落点保持删掉，登记簿原样留着，
        // 重跑时这些落点按「已撤回」跳过，因此重跑即可续完
        OnFailure::Resume,
        |paths| prepare_unuse(paths, key, anchors, only, force),
        |plan| project_outcome(key, true, plan.targets),
        |ctx, plan| run_unuse(ctx, key, plan),
        |milestones, elapsed_ms| {
            // 撤回完成是业务里程碑：逐目标各记一条，读日志即可重建「撤了哪几处」
            for (anchor, count) in milestones {
                anchor.log_milestone(key, count, elapsed_ms, "解除使用完成");
            }
        },
    )
}

/// 已持锁的执行段：计划已定，这里只落盘——删落点、改登记记录。
/// 交回的里程碑是逐目标的身份与删除数
fn run_unuse(
    ctx: &mut crate::write::Ctx<'_>,
    key: &str,
    plan: UnusePlan,
) -> Result<Applied<UnuseOutcome, Vec<(Anchor, usize)>>, ProjectError> {
    // 逐落点删；删到一半失败就停下，登记簿保持原样——已删的落点再跑一次会被当作
    // 「已撤回」跳过，所以重跑即可续完，不会留下登记簿与盘上不一致
    for target in &plan.targets {
        for dest in &target.destinations {
            if let Err(e) = link::remove_link(&dest.path, target.kind) {
                tracing::warn!(path = %dest.path.display(), cause = %e, "落点删除失败");
                return Err(e.into());
            }
        }
    }

    commit_registry(ctx.paths, ctx.token, |registry| {
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
        registry.prune_empty_projects();
        Ok(())
    })?;

    // 落盘成功后的机器提交：登记簿本身不进 git（ignored），提交只在
    // hub 工作区有变更时发生，消息只用 agent 名、键名与项目名
    let mut commits = Vec::new();
    if plan.targets.iter().any(|t| !t.destinations.is_empty()) {
        let agents = plan
            .targets
            .iter()
            .flat_map(|t| t.agents.iter().cloned())
            .collect::<Vec<_>>();
        let anchors: Vec<&Anchor> = plan.targets.iter().map(|t| &t.anchor).collect();
        commits.push(usage_message(Verb::Unuse, key, &agents, &anchors));
    }
    let milestones: Vec<(Anchor, usize)> = plan
        .targets
        .iter()
        .map(|target| (target.anchor.clone(), target.destinations.len()))
        .collect();
    Ok(Applied {
        outcome: project_outcome(key, false, plan.targets),
        commits,
        milestones,
    })
}

/// 结果投影：预演与实跑共用这一处。计划给出身份、agent、落点与动作清单，
/// 两者的差别只有一条：预演只报将执行的动作，实跑才报撤走的 agent、链接方式
/// 与真删的落点。载荷加字段只改这里
fn project_outcome(key: &str, dry_run: bool, targets: Vec<UnuseTargetPlan>) -> UnuseOutcome {
    UnuseOutcome {
        dry_run,
        key: key.to_string(),
        targets: targets
            .into_iter()
            .map(|target| UnuseTarget {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: if dry_run { Vec::new() } else { target.agents },
                link: if dry_run { None } else { Some(target.kind) },
                removed: if dry_run {
                    Vec::new()
                } else {
                    target
                        .destinations
                        .into_iter()
                        .map(|dest| dest.path.display().to_string())
                        .collect()
                },
                actions: if dry_run { target.actions } else { Vec::new() },
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
    /// 该技能记录登记的链接方式
    kind: LinkKind,
    /// 真正要删的落点（与登记 agent 现算）
    destinations: Vec<Dest>,
    /// 将执行的动作清单
    actions: Vec<PlanStep>,
}

/// 共用检查：各目标的记录都在；全部落点形态与登记一致，copy 未被修改。
/// 落点是整条记录解析出来的，因此保留方的 agent 也一并被解析——配置坏的保留方会被点名，
/// 共用落点也就有了判断依据。
fn prepare_unuse(
    paths: &HubPaths,
    key: &str,
    anchors: Vec<Anchor>,
    only: &[String],
    force: bool,
) -> Result<UnusePlan, ProjectError> {
    let registry = hub::load_registry(paths)?;
    // agent 清单只读一次，逐目标复用
    let known = agents::list(paths)?;
    let mut targets = Vec::new();
    for anchor in dedupe_anchors(anchors)? {
        let skill = registered_skill(&registry, &anchor, key)?;
        let withdrawn = withdrawn_agents(&skill.agents, key, only)?;
        // 落点靶子唯一：store 工作副本；占位由技能状态模块判定，不在场时报原生错误
        let source = skill_occupancy(paths, key).require_active()?;
        let resolved = match &anchor {
            Anchor::Project(dir) => resolve_project(&known, dir, &skill.agents, key)?,
            Anchor::Global => resolve_global(&known, &skill.agents, key)?,
        };
        // 共用落点：只有引用它的 agent 全在撤回名单里时才删，否则留给还在用的那一方
        let (doomed, kept): (Vec<Dest>, Vec<Dest>) = resolved
            .into_iter()
            .partition(|dest| dest.agents.iter().all(|agent| withdrawn.contains(agent)));
        // 保留是撤回里唯一的删除决策点：记下哪处落点被留下、还有谁在用，日志里才分得清
        for dest in &kept {
            tracing::debug!(path = %dest.path.display(), agents = %dest.agents.join(","), "共用落点保留");
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
            kind: skill.link,
            actions: destinations
                .iter()
                .map(|d| {
                    PlanStep::of(ActionKind::Remove, d.path.display().to_string())
                        .with_agents(d.agents.clone())
                })
                .collect(),
            destinations,
        });
    }
    Ok(UnusePlan { targets })
}

/// 本次撤走的 agent：名单里的每个 agent 都必须在记录里，否则点名该 agent 与
/// 已登记清单。名单按记录的登记顺序取，不在名单里的 agent 留作引用方。
/// 空名单是合法的空撤回：落点与登记都不动，记录原样留着。
fn withdrawn_agents(
    registered: &[String],
    key: &str,
    only: &[String],
) -> Result<Vec<String>, ProjectError> {
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
    destinations: &[Dest],
    kind: LinkKind,
    source: &Path,
    force: bool,
) -> Result<Vec<Dest>, ProjectError> {
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
            DestVerdict::Missing => tracing::warn!(path = %d.path.display(), "落点已撤回跳过"),
            DestVerdict::Live => live.push(d.clone()),
        }
    }
    Ok(live)
}
