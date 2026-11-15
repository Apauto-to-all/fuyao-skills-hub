//! 使用：把技能铺给一次调用里的全部目标（可选的全局目标 + 零到多个项目目标），
//! 每个目标各登记一条技能记录。落点一律直连 store 工作副本。
//! 预检与实跑同源——预演只报告动作，实跑按同一份计划落盘。
//! 全部目标一起预检：任一目标不合格则整次不动盘；执行中途失败回滚本次已建落点。

use std::path::{Path, PathBuf};

use crate::agents;
use crate::hub;
use crate::identity::display_path;
use crate::link::LinkKind;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::project::ProjectError;
use crate::project::dest::{Dest, ensure_parent_is_dir};
use crate::project::target::{anchors_or_reject, dedupe_anchors};
use crate::project::{Anchor, TargetOutcome, Targets, UseOutcome, skill_record};
use crate::skill::{Occupancy, occupancy, skill_occupancy};
use crate::write::{self, Applied, Ctx, OnFailure};

/// use 的业务数据：技能源目录与逐目标的预检结果
struct UseData {
    /// 技能源目录（store 工作副本）
    source: PathBuf,
    /// 逐目标的预检结果，全局目标在最前
    targets: Vec<TargetPlan>,
    /// 收尾步骤：登记这类在全部落点之后执行的步骤
    tail: Vec<PlanStep>,
}

/// 一个目标的预检结果与执行结果
struct TargetPlan {
    /// 目标身份
    anchor: Anchor,
    /// 去重后的 agent 键
    agents: Vec<String>,
    /// 去重后的落点
    destinations: Vec<Dest>,
    /// 本目标的步骤：预演清单直接取这一份，不按计划位置切
    steps: Vec<PlanStep>,
    /// 本目标在计划里的落点组号：同组落点共用一个档位
    group: usize,
    /// 实际采用的链接方式：预检时未知，落盘后才有值
    link: Option<LinkKind>,
}

/// 共用预检：技能侧一次（存在性），再逐目标检查（登记冲突、落点空闲、父目录可用）。
/// `anchors` 里的项目目标是调用方给的原始写法，这里规范成绝对路径并去重。
/// 计划照目标的顺序排步骤，逐目标的步骤段记进该目标的业务数据
fn prepare_use(
    paths: &HubPaths,
    key: &str,
    agents: &[String],
    anchors: Vec<Anchor>,
) -> Result<Plan<UseData>, ProjectError> {
    // 占位由技能状态模块一次取全，这里的判定只剩「要什么」：使用只接受带工作副本的技能，
    // 返回的工作副本目录即唯一源
    let source = skill_occupancy(paths, key).require_active()?;
    let mut agent_keys: Vec<String> = Vec::new();
    for a in agents {
        if !agent_keys.iter().any(|e| e == a) {
            agent_keys.push(a.clone());
        }
    }
    // 一次使用至少给一个 agent：空名单没有可铺的落点，直接拒绝
    if agent_keys.is_empty() {
        return Err(ProjectError::NoAgent);
    }
    // 登记簿与 agent 清单各读一次，逐目标复用
    let registry = hub::load_registry(paths)?;
    let known = agents::list(paths)?;
    let mut plan = Plan::new(UseData {
        source: source.clone(),
        targets: Vec::new(),
        tail: Vec::new(),
    });
    for anchor in dedupe_anchors(anchors)? {
        // 登记闸门：该目标已有此技能的记录时拒绝，只新建、不覆盖已有记录
        if skill_record(&registry, &anchor, key).is_some() {
            return Err(match &anchor {
                Anchor::Project(dir) => ProjectError::AlreadyRegistered {
                    key: key.into(),
                    project: display_path(dir),
                },
                Anchor::Global => ProjectError::AlreadyRegisteredGlobal { key: key.into() },
            });
        }
        let destinations = anchor.resolve(&known, &agent_keys, key)?;
        // 全部落点必须空闲；父目录链上有同名文件同样算占用
        for dest in &destinations {
            let occupied = occupancy(&dest.path);
            if occupied != Occupancy::Nothing {
                return Err(ProjectError::DestinationOccupied {
                    path: dest.path.display().to_string(),
                    kind: occupied.to_string(),
                });
            }
            ensure_parent_is_dir(&dest.path)?;
        }
        // 组号即目标在计划里的序：同组落点共用第一个落点定下的档位
        let group = plan.data.targets.len();
        let mut steps = Vec::new();
        for dest in &destinations {
            let desc = PlanStep::between(
                ActionKind::Link,
                source.display().to_string(),
                dest.path.display().to_string(),
            )
            .targeting(key)
            .with_agents(dest.agents.clone());
            steps.push(desc.clone());
            plan.push(
                desc,
                Act::Link {
                    from: source.clone(),
                    to: dest.path.clone(),
                    group,
                },
            );
        }
        // 登记是收尾步骤：全部落点建完才写登记簿，计划里因此排在所有目标之后
        plan.data.tail.push(match &anchor {
            Anchor::Project(dir) => {
                PlanStep::of(ActionKind::Register, key).landed_at(display_path(dir))
            }
            Anchor::Global => PlanStep::of(ActionKind::Register, key).landed_at("global"),
        });
        plan.data.targets.push(TargetPlan {
            anchor,
            agents: agent_keys.clone(),
            destinations,
            steps,
            group,
            link: None,
        });
    }
    if plan.data.targets.is_empty() {
        return Err(ProjectError::NoTarget);
    }
    // 收尾步骤排在全部落点之后：计划顺序即执行顺序
    for desc in plan.data.tail.clone() {
        plan.push(desc, Act::None);
    }
    Ok(plan)
}

/// 一次使用：只有在场 skill、各目标都无登记记录、全部落点空闲时才执行。
/// `agents` 中的重复项去重后登记；同一实际落点只建立一次入口。
/// dry_run = true 时预演：只读不落盘、不取写锁——别人正在写也能拿到动作清单。
pub fn use_skill(
    hub_root: &Path,
    key: &str,
    agents: &[String],
    targets: Targets,
    dry_run: bool,
) -> Result<UseOutcome, ProjectError> {
    // 入参判定先于装配：键形状与目标集合都不依赖 hub
    crate::key::validate(key)?;
    let anchors = anchors_or_reject(&targets)?;
    write::run(
        hub_root,
        dry_run,
        // 建落点失败要撤干净：盘上不留没有记录的入口
        OnFailure::Rollback,
        |paths| prepare_use(paths, key, agents, anchors),
        |plan, command| {
            // 命令级步骤（手改快照）挂顶层，逐目标步骤随目标带出，收尾步骤单列
            let UseData { targets, tail, .. } = plan.data;
            project_outcome(key, true, targets, tail, command)
        },
        |ctx, plan| run_use(ctx, key, plan),
        |milestones, elapsed_ms| {
            // 使用完成是业务里程碑：逐目标各记一条，读日志即可重建「哪个目标铺了什么」
            for (anchor, count) in milestones {
                anchor.log_milestone(key, count, elapsed_ms, "使用完成");
            }
        },
    )
}

/// 已持锁的落盘段：落点已由骨架按计划建出，这里收口链接闸并写登记记录。
/// 交回的里程碑是逐目标的身份与落点数，报告本身即这份清单
fn run_use(
    ctx: &mut Ctx<'_>,
    key: &str,
    plan: Plan<UseData>,
) -> Result<Applied<UseOutcome, Vec<(Anchor, usize)>>, ProjectError> {
    let UseData {
        source,
        mut targets,
        tail: _,
    } = plan.data;
    // 档位按目标收口：落盘时组内第一个落点定档、其余照它建，
    // 这里取的就是那份事实——登记记录的一个 link 值与盘上形态因此必然一致
    for target in &mut targets {
        target.link = ctx.group_kind(target.group);
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
    // 登记簿：各目标一次写入，登记顺序规则住登记簿侧；
    // 预检已把项目根规范成绝对路径，登记簿与落点因此共用同一身份
    ctx.edit_registry(|registry| -> Result<(), ProjectError> {
        for target in &targets {
            let Some(kind) = target.link else { continue };
            let record = crate::hub::ProjectSkill {
                key: key.to_string(),
                agents: target.agents.clone(),
                link: kind,
            };
            registry.record_use(&target.anchor, record, crate::hub::unix_now());
        }
        Ok(())
    })?;
    // 落点建在项目与 agent 的目录里，登记簿是 ignored 文件：hub 仓库没有可提交的变更，
    // 本命令的步骤里因此没有提交步
    let milestones: Vec<(Anchor, usize)> = targets
        .iter()
        .map(|target| (target.anchor.clone(), target.destinations.len()))
        .collect();
    Ok(Applied {
        outcome: project_outcome(key, false, targets, Vec::new(), Vec::new()),
        milestones,
    })
}

/// 结果投影：预演与实跑共用这一处。计划给出身份、agent、落点与动作清单，
/// 两者的差别只有一条：链接方式要真建入口才有值，因此实跑带 `link`、不带动作清单，
/// 预演反之。载荷加字段只改这里
fn project_outcome(
    key: &str,
    dry_run: bool,
    targets: Vec<TargetPlan>,
    tail: Vec<PlanStep>,
    command: Vec<PlanStep>,
) -> UseOutcome {
    UseOutcome {
        dry_run,
        key: key.to_string(),
        actions: command,
        tail,
        targets: targets
            .into_iter()
            .map(|target| {
                let actions = if dry_run { target.steps } else { Vec::new() };
                TargetOutcome {
                    scope: target.anchor.scope(),
                    project: target.anchor.project(),
                    agents: target.agents,
                    link: target.link,
                    destinations: target.destinations.into_iter().map(Into::into).collect(),
                    actions,
                }
            })
            .collect(),
    }
}
