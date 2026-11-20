//! 使用：把一整套技能铺给一次调用里的全部目标（可选的全局目标 + 零到多个
//! 项目目标），每个技能各自建落点、各登记一条记录。落点一律直连 store 工作副本。
//! 已有落点或登记记录的技能跳过并报告，其余照常；链接方式按目标收口，同一目标
//! 的落点共用一次档位判定。
//! 预检与实跑同源——预演只报告动作，实跑按同一份计划落盘。
//! 整批一起预检：任一技能在任一目标上不合格即跳过该技能，其余不动；执行中途
//! 失败回滚本次已建落点。

use std::collections::HashSet;
use std::path::Path;

use crate::agents;
use crate::hub;
use crate::identity::{display_path, path_identity};
use crate::link::LinkKind;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::project::ProjectError;
use crate::project::dest::{Dest, ensure_parent_is_dir};
use crate::project::target::{anchors_or_reject, dedupe_anchors};
use crate::project::{Anchor, Targets, skill_record};
use crate::select::{self, Selector};
use crate::skill::{Lists, Occupancy, SelectionOutcome, Skipped, dry, occupancy, skill_occupancy};
use crate::write::{self, Applied, Ctx, OnFailure};

/// use 的业务数据：做掉的键、跳过的项与逐目标的落点事实
struct UseSet {
    /// 本次做掉的键，按选择顺序
    applied: Vec<String>,
    /// 预期内不适用而跳过的键
    skipped: Vec<Skipped>,
    /// 逐目标的落点事实，全局目标在最前
    targets: Vec<TargetPlan>,
}

impl Lists for UseSet {
    fn lists(self) -> (Vec<String>, Vec<Skipped>) {
        (self.applied, self.skipped)
    }
}

/// 一个目标铺下的全部落点：档位按目标收口，记录按技能逐条写
struct TargetPlan {
    /// 目标身份
    anchor: Anchor,
    /// 去重后的 agent 键
    agents: Vec<String>,
    /// 本目标在计划里的落点组号：同组落点共用一个档位
    group: usize,
    /// 本目标铺下的技能键，按选择顺序
    keys: Vec<String>,
    /// 本目标的落点总数
    count: usize,
    /// 实际采用的链接方式：预检时未知，落盘后才有值
    link: Option<LinkKind>,
}

/// 共用预检：选择器解析出键，逐技能在全部目标上查登记与落点占用。
/// 冲突只跳过这一个技能并报告，不让整批陪葬；全部通过后计划一次成形
fn prepare_use(
    paths: &HubPaths,
    selector: &Selector,
    agents: &[String],
    anchors: Vec<Anchor>,
) -> Result<Plan<UseSet>, ProjectError> {
    // 一次使用至少给一个 agent：空名单没有可铺的落点
    let mut agent_keys: Vec<String> = Vec::new();
    for agent in agents {
        if !agent_keys.iter().any(|kept| kept == agent) {
            agent_keys.push(agent.clone());
        }
    }
    if agent_keys.is_empty() {
        return Err(ProjectError::NoAgent);
    }
    // 候选：store 面里该前缀下的全部技能；选择器只从这里认名字
    let available = select::members(
        crate::skill::walk_face(&paths.store())
            .filter(|skill| skill.has_skill_md)
            .map(|skill| skill.key),
        selector.prefix(),
    );
    let keys = selector.resolve(&available)?;
    let anchors = dedupe_anchors(anchors)?;
    // 登记簿与 agent 清单各读一次，逐技能逐目标复用
    let registry = hub::load_registry(paths)?;
    let known = agents::list(paths)?;
    let mut plan = Plan::new(UseSet {
        applied: Vec::new(),
        skipped: Vec::new(),
        targets: anchors
            .iter()
            .enumerate()
            .map(|(group, anchor)| TargetPlan {
                anchor: anchor.clone(),
                agents: agent_keys.clone(),
                group,
                keys: Vec::new(),
                count: 0,
                link: None,
            })
            .collect(),
    });
    // 本批已排进计划的落点：同一套里末段重名的两个技能会争同一处落点，
    // 后者按落点已占用跳过，不让整批在同一处建两遍
    let mut planned: HashSet<String> = HashSet::new();
    for key in &keys {
        let source = skill_occupancy(paths, key).require_active()?;
        // 先把该技能在全部目标上的冲突查完：任一目标已登记或落点被占，整个技能跳过
        let mut destinations: Vec<(usize, Vec<Dest>)> = Vec::new();
        let mut conflict: Option<ProjectError> = None;
        for (group, anchor) in anchors.iter().enumerate() {
            // 登记闸门：该目标已有此技能的记录时跳过，只新建、不覆盖已有记录
            if skill_record(&registry, anchor, key).is_some() {
                conflict = Some(match anchor {
                    Anchor::Project(dir) => ProjectError::AlreadyRegistered {
                        key: key.clone(),
                        project: display_path(dir),
                    },
                    Anchor::Global => ProjectError::AlreadyRegisteredGlobal { key: key.clone() },
                });
                break;
            }
            let resolved = anchor.resolve(&known, &agent_keys, key)?;
            if let Some(e) = resolved
                .iter()
                .find_map(|dest| dest_conflict(dest, &planned))
            {
                conflict = Some(e);
                break;
            }
            destinations.push((group, resolved));
        }
        if let Some(e) = conflict {
            plan.data.skipped.push(Skipped::of(key, &e));
            continue;
        }
        plan.data.applied.push(key.clone());
        for (group, resolved) in destinations {
            let count = resolved.len();
            {
                let target = &mut plan.data.targets[group];
                target.keys.push(key.clone());
                target.count += count;
            }
            for dest in resolved {
                planned.insert(path_identity(&dest.path));
                plan.push(
                    PlanStep::between(
                        ActionKind::Link,
                        source.display().to_string(),
                        dest.path.display().to_string(),
                    )
                    .targeting(key)
                    .with_agents(dest.agents.clone()),
                    Act::Link {
                        from: source.clone(),
                        to: dest.path.clone(),
                        group,
                    },
                );
            }
        }
    }
    // 登记是收尾步骤：全部落点建完才写登记簿，计划里因此排在所有落点之后
    let mut tail: Vec<PlanStep> = Vec::new();
    for target in &plan.data.targets {
        for key in &target.keys {
            tail.push(match &target.anchor {
                Anchor::Project(dir) => {
                    PlanStep::of(ActionKind::Register, key).landed_at(display_path(dir))
                }
                Anchor::Global => PlanStep::of(ActionKind::Register, key).landed_at("global"),
            });
        }
    }
    for step in tail {
        plan.push(step, Act::None);
    }
    Ok(plan)
}

/// 一处落点能否落脚：父目录链上有同名文件、落点已被占、或本次已排进计划，
/// 都按落点被占报告。这些是预期内的「不适用」，由调用方跳过该技能
fn dest_conflict(dest: &Dest, planned: &HashSet<String>) -> Option<ProjectError> {
    if let Err(e) = ensure_parent_is_dir(&dest.path) {
        return Some(e);
    }
    if !planned.contains(&path_identity(&dest.path)) {
        let occupied = occupancy(&dest.path);
        if occupied == Occupancy::Nothing {
            return None;
        }
        return Some(ProjectError::DestinationOccupied {
            path: dest.path.display().to_string(),
            kind: occupied.to_string(),
        });
    }
    Some(ProjectError::DestinationOccupied {
        path: dest.path.display().to_string(),
        kind: "同一批里已排定的落点".to_string(),
    })
}

/// 一次使用：按选择器把整套技能铺给各目标。
/// `agents` 中的重复项去重后登记；同一实际落点只建立一次入口。
/// dry_run = true 时预演：只读不落盘、不取写锁——别人正在写也能拿到动作清单。
pub fn use_skill(
    hub_root: &Path,
    selector: &Selector,
    agents: &[String],
    targets: Targets,
    dry_run: bool,
) -> Result<SelectionOutcome, ProjectError> {
    // 形状判定先于装配：键写错当场点名，目标集合不依赖 hub
    selector.check()?;
    let anchors = anchors_or_reject(&targets)?;
    write::run(
        hub_root,
        dry_run,
        // 建落点失败要撤干净：盘上不留没有记录的入口
        OnFailure::Rollback,
        |paths| prepare_use(paths, selector, agents, anchors),
        // 预演与实跑同出一份计划：命令级步骤挂顶层，计划步骤按执行顺序接上
        dry,
        run_use,
        |milestones, elapsed_ms| {
            // 使用完成是业务里程碑：逐目标各记一条，读日志即可重建「哪个目标铺了几个落点」
            for (anchor, count) in milestones {
                anchor.log_milestone(selector.label(), count, elapsed_ms, "使用完成");
            }
        },
    )
}

/// 已持锁的落盘段：落点已由骨架按计划建出，这里收口链接闸并写登记记录。
/// 交回的里程碑是逐目标的身份与落点数，报告本身即这份清单
fn run_use(
    ctx: &mut Ctx<'_>,
    plan: Plan<UseSet>,
) -> Result<Applied<SelectionOutcome, Vec<(Anchor, usize)>>, ProjectError> {
    let UseSet {
        applied,
        skipped,
        mut targets,
    } = plan.data;
    // 档位按目标收口：落盘时组内第一个落点定档、其余照它建，
    // 这里取的就是那份事实——登记记录的一个 link 值与盘上形态因此必然一致
    for target in &mut targets {
        target.link = ctx.group_kind(target.group);
    }
    for target in &targets {
        let Some(kind) = target.link else { continue };
        let at = target
            .anchor
            .project()
            .unwrap_or_else(|| "global".to_string());
        if kind == LinkKind::Copy {
            // 档位决定落点后续怎么维护：copy 不与源同步，是链接降级，必须可查
            tracing::warn!(project = %at, count = target.count, "落点降级");
        } else {
            // 链接方式选择是诊断细节：DEBUG 记一次选定，排查落点形态时对照
            tracing::debug!(project = %at, count = target.count, "链接方式选定");
        }
    }
    // 登记簿：各目标逐技能写入，登记顺序规则住登记簿侧；
    // 预检已把项目根规范成绝对路径，登记簿与落点因此共用同一身份
    ctx.edit_registry(|registry| -> Result<(), ProjectError> {
        for target in &targets {
            let Some(kind) = target.link else { continue };
            for key in &target.keys {
                let record = hub::ProjectSkill {
                    key: key.clone(),
                    agents: target.agents.clone(),
                    link: kind,
                };
                registry.record_use(&target.anchor, record, hub::unix_now());
            }
        }
        Ok(())
    })?;
    // 落点建在项目与 agent 的目录里，登记簿是 ignored 文件：hub 仓库没有可提交的变更，
    // 本命令的步骤里因此没有提交步
    let milestones: Vec<(Anchor, usize)> = targets
        .iter()
        .map(|target| (target.anchor.clone(), target.count))
        .collect();
    Ok(Applied {
        outcome: SelectionOutcome {
            applied,
            skipped,
            verb: Some(crate::commit::Verb::Use),
            ..SelectionOutcome::default()
        },
        milestones,
    })
}
