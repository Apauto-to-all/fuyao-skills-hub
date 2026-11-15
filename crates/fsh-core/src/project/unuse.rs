//! 撤回：把某技能在一次调用里的全部目标（可选的全局目标 + 零到多个项目目标）
//! 的落点撤走，并从对应的登记记录里摘掉点名的 agent；记录里没有别的 agent 时
//! 连记录一起删，名单点满整条记录即整条撤回。
//! 全部目标先一起检查，任一目标不合格则整个操作不动盘。

use std::path::Path;

use super::dest::{Dest, DestGates, DestVerdict, check_destination};
use super::target::{anchors_or_reject, dedupe_anchors};
use super::{Anchor, ProjectError, Targets, UnuseOutcome, UnuseTarget, registered_skill};
use crate::agents;
use crate::hub;
use crate::link::LinkKind;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
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
        |plan, command| {
            // 命令级步骤（手改快照）挂顶层，逐目标步骤随目标带出
            project_outcome(key, true, plan.data.targets, command)
        },
        |ctx, plan| run_unuse(ctx, key, plan),
        |milestones, elapsed_ms| {
            // 撤回完成是业务里程碑：逐目标各记一条，读日志即可重建「撤了哪几处」
            for (anchor, count) in milestones {
                anchor.log_milestone(key, count, elapsed_ms, "解除使用完成");
            }
        },
    )
}

/// 已持锁的落盘段：落点已由骨架按计划删掉，这里只改登记记录。
/// 删到一半失败时登记簿保持原样——已删的落点再跑一次会被当作「已撤回」跳过，
/// 所以重跑即可续完，不会留下登记簿与盘上不一致。
/// 交回的里程碑是逐目标的身份与删除数
fn run_unuse(
    ctx: &mut crate::write::Ctx<'_>,
    key: &str,
    plan: Plan<UnuseData>,
) -> Result<Applied<UnuseOutcome, Vec<(Anchor, usize)>>, ProjectError> {
    let targets = plan.data.targets;
    ctx.edit_registry(|registry| -> Result<(), ProjectError> {
        for target in &targets {
            // 名单点满整条记录即整条撤回；否则只把名单里的名字摘掉
            let remaining = if target.remaining.is_empty() {
                None
            } else {
                Some(target.remaining.as_slice())
            };
            registry.remove_use(&target.anchor, key, remaining);
        }
        Ok(())
    })?;

    // 落点与登记簿都不在 hub 仓库里（登记簿是 ignored 文件），本次没有可提交的变更
    let milestones: Vec<(Anchor, usize)> = targets
        .iter()
        .map(|target| (target.anchor.clone(), target.destinations.len()))
        .collect();
    Ok(Applied {
        outcome: project_outcome(key, false, targets, Vec::new()),
        milestones,
    })
}

/// 结果投影：预演与实跑共用这一处。计划给出身份、agent、落点与动作清单，
/// 两者的差别只有一条：预演只报将执行的动作，实跑才报撤走的 agent、链接方式
/// 与真删的落点。载荷加字段只改这里
fn project_outcome(
    key: &str,
    dry_run: bool,
    targets: Vec<UnuseTargetPlan>,
    command: Vec<PlanStep>,
) -> UnuseOutcome {
    UnuseOutcome {
        dry_run,
        key: key.to_string(),
        actions: command,
        targets: targets
            .into_iter()
            .map(|target| {
                let actions = if dry_run { target.steps } else { Vec::new() };
                UnuseTarget {
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
                    actions,
                }
            })
            .collect(),
    }
}

/// unuse 的业务数据：逐目标的检查结果
struct UnuseData {
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
    /// 本目标的步骤：预演清单直接取这一份，不按计划位置切
    steps: Vec<PlanStep>,
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
) -> Result<Plan<UnuseData>, ProjectError> {
    let registry = hub::load_registry(paths)?;
    // agent 清单只读一次，逐目标复用
    let known = agents::list(paths)?;
    let mut plan = Plan::new(UnuseData {
        targets: Vec::new(),
    });
    for anchor in dedupe_anchors(anchors)? {
        let skill = registered_skill(&registry, &anchor, key)?;
        let withdrawn = withdrawn_agents(&skill.agents, key, only)?;
        // 落点靶子唯一：store 工作副本；占位由技能状态模块判定，不在场时报原生错误
        let source = skill_occupancy(paths, key).require_active()?;
        let resolved = anchor.resolve(&known, &skill.agents, key)?;
        // 共用落点：只有引用它的 agent 全在撤回名单里时才删，否则留给还在用的那一方
        let (doomed, kept): (Vec<Dest>, Vec<Dest>) = resolved
            .into_iter()
            .partition(|dest| dest.agents.iter().all(|agent| withdrawn.contains(agent)));
        // 保留是撤回里唯一的删除决策点：记下哪处落点被留下、还有谁在用，日志里才分得清
        for dest in &kept {
            tracing::debug!(path = %dest.path.display(), agents = %dest.agents.join(","), "共用落点保留");
        }
        let destinations = check_destinations(&doomed, skill.link, &source, force)?;
        let mut steps = Vec::new();
        for dest in &destinations {
            let desc = PlanStep::of(ActionKind::Remove, dest.path.display().to_string())
                .with_agents(dest.agents.clone());
            steps.push(desc.clone());
            plan.push(
                desc,
                Act::Unlink {
                    path: dest.path.clone(),
                    kind: skill.link,
                },
            );
        }
        plan.data.targets.push(UnuseTargetPlan {
            anchor,
            remaining: skill
                .agents
                .iter()
                .filter(|agent| !withdrawn.contains(agent))
                .cloned()
                .collect(),
            agents: withdrawn,
            kind: skill.link,
            destinations,
            steps,
        });
    }
    Ok(plan)
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
