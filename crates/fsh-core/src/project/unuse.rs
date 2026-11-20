//! 撤回：把一整套技能在一次调用里的全部目标（可选的全局目标 + 零到多个项目目标）
//! 的落点撤走，并从对应的登记记录里摘掉点名的 agent；记录里没有别的 agent 时
//! 连记录一起删，名单点满整条记录即整条撤回。
//! 一次撤回一整套：没有可撤记录的技能跳过并报告，其余照常撤；单个键仍按既有
//! 规则整键判定——任一目标不合格则这个键不动盘。

use std::path::Path;

use super::dest::{Dest, DestGates, DestVerdict, check_destination};
use super::target::{anchors_or_reject, dedupe_anchors};
use super::{Anchor, ProjectError, Targets, registered_skill};
use crate::agents;
use crate::hub;
use crate::link::LinkKind;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::select::{self, Selector};
use crate::skill::{Lists, SelectionOutcome, Skipped, dry, face_keys, skill_occupancy};
use crate::write::{self, Applied, OnFailure};

/// 撤回一整套：按选择器解析出键，逐键从每个目标的记录里摘掉点名的 agent，
/// 记录里没有别的 agent 时连记录一起删；共用落点留到引用它的 agent 全在本次
/// 名单里时才删。没有可撤记录的键进跳过清单，不堵住其余技能；`force` 只覆盖
/// 「copy 内容被修改」。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn unuse(
    hub_root: &Path,
    selector: &Selector,
    targets: Targets,
    only: &[String],
    force: bool,
    dry_run: bool,
) -> Result<SelectionOutcome, ProjectError> {
    // 入参判定先于装配：选择器形状与目标集合都不依赖 hub
    selector.check()?;
    let anchors = anchors_or_reject(&targets)?;
    write::run(
        hub_root,
        dry_run,
        // 删除不回滚：删到一半失败时已删的落点保持删掉，登记簿原样留着，
        // 重跑时这些落点按「已撤回」跳过，因此重跑即可续完
        OnFailure::Resume,
        |paths| prepare_unuse(paths, selector, anchors, only, force),
        // 命令级步骤（手改快照）在最前，整套的步骤按计划顺序接上；
        // 预演与实跑同出一份计划，跳过项两边都逐条列出
        dry,
        run_unuse,
        |count, elapsed_ms| {
            // 撤回跨多个目标与多条登记改写：一条 INFO 交代整套、条数与整段耗时
            tracing::info!(
                prefix = %selector.label(),
                count,
                elapsed_ms = elapsed_ms,
                "解除使用完成"
            );
        },
    )
}

/// 已持锁的落盘段：落点已由骨架按计划删掉，这里只改登记记录。
/// 删到一半失败时登记簿保持原样——已删的落点再跑一次会被当作「已撤回」跳过，
/// 所以重跑即可续完，不会留下登记簿与盘上不一致。
/// 交回的里程碑是本次做掉的键数
fn run_unuse(
    ctx: &mut crate::write::Ctx<'_>,
    plan: Plan<UnuseSet>,
) -> Result<Applied<SelectionOutcome, usize>, ProjectError> {
    let UnuseSet {
        applied,
        skipped,
        targets,
    } = plan.data;
    ctx.edit_registry(|registry| -> Result<(), ProjectError> {
        for target in &targets {
            // 名单点满整条记录即整条撤回；否则只把名单里的名字摘掉
            let remaining = if target.remaining.is_empty() {
                None
            } else {
                Some(target.remaining.as_slice())
            };
            registry.remove_use(&target.anchor, &target.key, remaining);
        }
        Ok(())
    })?;
    let count = applied.len();
    Ok(Applied {
        outcome: SelectionOutcome {
            applied,
            skipped,
            ..SelectionOutcome::default()
        },
        milestones: count,
    })
}

/// 一次撤回的业务数据：做掉的键、跳过的键与要改的登记记录
#[derive(Debug, Default)]
struct UnuseSet {
    /// 本次做掉的键
    applied: Vec<String>,
    /// 预期内不适用而跳过的键
    skipped: Vec<Skipped>,
    /// 逐目标的登记改写：每个做掉的键在每个目标上各一条
    targets: Vec<UnuseTargetPlan>,
}

impl Lists for UnuseSet {
    fn lists(self) -> (Vec<String>, Vec<Skipped>) {
        (self.applied, self.skipped)
    }
}

/// 一个目标上的撤回结果：改哪条记录，留下哪些 agent，删哪些落点
#[derive(Debug)]
struct UnuseTargetPlan {
    /// 目标身份
    anchor: Anchor,
    /// 技能键
    key: String,
    /// 记录里剩下的 agent；空表示整条记录撤回
    remaining: Vec<String>,
    /// 该记录登记的链接方式
    kind: LinkKind,
    /// 真正要删的落点（与登记 agent 现算）
    destinations: Vec<Dest>,
}

/// 撤回预检：解析选择 → 逐键过既有判定。没有可撤记录的键与工作副本已不在场的
/// 键进跳过清单，其余不合格（目标目录、agent 配置、落点形态、copy 内容）整批在动盘前失败。
fn prepare_unuse(
    paths: &HubPaths,
    selector: &Selector,
    anchors: Vec<Anchor>,
    only: &[String],
    force: bool,
) -> Result<Plan<UnuseSet>, ProjectError> {
    let registry = hub::load_registry(paths)?;
    // 候选 = 工作副本面里的技能 ∪ 登记簿里的键：前者让「从没用过」的技能也能被
    // 点名并进跳过清单，后者让工作副本被手工删掉的键仍能被点名、按不适用跳过
    let available = select::members(
        face_keys(&paths.store())
            .chain(registry.records().map(|record| record.skill().key.clone())),
        selector.prefix(),
    );
    let mut keys = selector.resolve(&available)?;
    // 同一个键可能同时在两个来源里；available 已排序，相邻去重即可
    keys.dedup();
    // agent 清单只读一次，逐目标复用
    let known = agents::list(paths)?;
    let anchors = dedupe_anchors(anchors)?;
    let mut plan = Plan::new(UnuseSet::default());
    for key in keys {
        match check_key(paths, &registry, &known, &anchors, &key, only, force) {
            Ok(units) => {
                for unit in units {
                    for dest in &unit.destinations {
                        let desc =
                            PlanStep::of(ActionKind::Remove, dest.path.display().to_string())
                                .with_agents(dest.agents.clone());
                        plan.push(
                            desc,
                            Act::Unlink {
                                path: dest.path.clone(),
                                kind: unit.kind,
                            },
                        );
                    }
                    plan.data.targets.push(unit);
                }
                plan.data.applied.push(key);
            }
            // 预期内的不适用：跳过并报告，不堵住其余技能
            Err(e) if skippable(&e) => plan.data.skipped.push(Skipped::of(&key, &e)),
            Err(e) => return Err(e),
        }
    }
    Ok(plan)
}

/// 一个键在全目标的检查结果：逐目标确认记录在场、名单可撤、落点可删。
/// 任一目标不合格即整键退回，由调用方决定是跳过还是整批失败
fn check_key(
    paths: &HubPaths,
    registry: &hub::ProjectRegistry,
    known: &[agents::AgentInfo],
    anchors: &[Anchor],
    key: &str,
    only: &[String],
    force: bool,
) -> Result<Vec<UnuseTargetPlan>, ProjectError> {
    let mut units = Vec::new();
    for anchor in anchors {
        let skill = registered_skill(registry, anchor, key)?;
        let withdrawn = withdrawn_agents(&skill.agents, key, only)?;
        // 落点靶子唯一：store 工作副本；占位由技能状态模块判定，不在场时
        // 由调用方按「这个键撤不动」跳过
        let source = skill_occupancy(paths, key).require_active()?;
        let resolved = anchor.resolve(known, &skill.agents, key)?;
        // 共用落点：只有引用它的 agent 全在撤回名单里时才删，否则留给还在用的那一方
        let (doomed, kept): (Vec<Dest>, Vec<Dest>) = resolved
            .into_iter()
            .partition(|dest| dest.agents.iter().all(|agent| withdrawn.contains(agent)));
        // 保留是撤回里唯一的删除决策点：记下哪处落点被留下、还有谁在用，日志里才分得清
        for dest in &kept {
            tracing::debug!(path = %dest.path.display(), agents = %dest.agents.join(","), "共用落点保留");
        }
        units.push(UnuseTargetPlan {
            anchor: anchor.clone(),
            key: key.to_string(),
            remaining: skill
                .agents
                .iter()
                .filter(|agent| !withdrawn.contains(agent))
                .cloned()
                .collect(),
            kind: skill.link,
            destinations: check_destinations(&doomed, skill.link, &source, force)?,
        });
    }
    Ok(units)
}

/// 没有可撤记录：该目标没有这条技能的登记、名单里的 agent 不在记录里，
/// 或登记还在而工作副本被手工删掉（这个键撤不动）。这些都是预期内的不适用，
/// 跳过并报告，不堵住其余技能；其余错误整批在动盘前失败
fn skippable(e: &ProjectError) -> bool {
    matches!(
        e,
        ProjectError::NotRegistered { .. }
            | ProjectError::NotRegisteredGlobal { .. }
            | ProjectError::AgentNotRegistered { .. }
            | ProjectError::SkillNotFound { .. }
    )
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
