//! 使用：把技能铺给一次调用里的全部目标（可选的全局目标 + 零到多个项目目标），
//! 每个目标各登记一条技能记录。落点一律直连 store 工作副本。
//! 预检与实跑同源——预演只报告动作，实跑按同一份计划落盘。
//! 全部目标一起预检：任一目标不合格则整次不动盘；执行中途失败回滚本次已建落点。

use std::path::{Path, PathBuf};

use super::commit_registry;
use crate::agents;
use crate::hub;
use crate::identity::display_path;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep, Steps};
use crate::project::ProjectError;
use crate::project::dest::{Dest, ensure_parent_is_dir, resolve_global, resolve_project};
use crate::project::target::{anchors_or_reject, dedupe_anchors};
use crate::project::{Anchor, TargetOutcome, Targets, UseOutcome, skill_record};
use crate::skill::skill_occupancy;
use crate::write::{self, Applied, Ctx, OnFailure};

/// use 的公共预检结果：实跑与预演由此同源
struct UsePlan {
    /// 技能源目录（store 工作副本）
    source: PathBuf,
    /// 逐目标的预检结果，全局目标在最前
    targets: Vec<TargetPlan>,
}

impl Steps for UsePlan {
    fn steps(&self) -> Vec<PlanStep> {
        self.targets
            .iter()
            .flat_map(|target| target.actions.clone())
            .collect()
    }
}

/// 一个目标的预检结果与执行结果
struct TargetPlan {
    /// 目标身份
    anchor: Anchor,
    /// 去重后的 agent 键
    agents: Vec<String>,
    /// 去重后的落点
    destinations: Vec<Dest>,
    /// 预演动作清单
    actions: Vec<PlanStep>,
    /// 实际采用的链接方式：预检时未知，建完入口才有值
    link: Option<LinkKind>,
}

/// 共用预检：技能侧一次（存在性），再逐目标检查（登记冲突、落点空闲、父目录可用）。
/// `anchors` 里的项目目标是调用方给的原始写法，这里规范成绝对路径并去重
fn prepare_use(
    paths: &HubPaths,
    key: &str,
    agents: &[String],
    anchors: Vec<Anchor>,
) -> Result<UsePlan, ProjectError> {
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
    let mut targets: Vec<TargetPlan> = Vec::new();
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
        let destinations = match &anchor {
            Anchor::Project(dir) => resolve_project(&known, dir, &agent_keys, key)?,
            Anchor::Global => resolve_global(&known, &agent_keys, key)?,
        };
        // 全部落点必须空闲；父目录链上有同名文件同样算占用
        for dest in &destinations {
            if dest.path.as_os_str().is_empty() || dest.path.symlink_metadata().is_ok() {
                return Err(ProjectError::DestinationOccupied {
                    path: dest.path.display().to_string(),
                });
            }
            ensure_parent_is_dir(&dest.path)?;
        }
        let mut actions = Vec::new();
        for dest in &destinations {
            actions.push(
                PlanStep::between(
                    ActionKind::Link,
                    source.display().to_string(),
                    dest.path.display().to_string(),
                )
                .targeting(key)
                .with_agents(dest.agents.clone()),
            );
        }
        actions.push(match &anchor {
            Anchor::Project(dir) => {
                PlanStep::of(ActionKind::Register, key).landed_at(display_path(dir))
            }
            Anchor::Global => PlanStep::of(ActionKind::Register, key).landed_at("global"),
        });
        targets.push(TargetPlan {
            anchor,
            agents: agent_keys.clone(),
            destinations,
            actions,
            link: None,
        });
    }
    if targets.is_empty() {
        return Err(ProjectError::NoTarget);
    }
    Ok(UsePlan { source, targets })
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
        |dry| {
            // 命令级步骤（手改快照）挂顶层，逐目标步骤留在各自目标里
            let crate::write::DryPlan {
                plan,
                preamble,
                steps: _,
            } = dry;
            project_outcome(key, true, plan.targets, preamble)
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
/// 已持锁的执行段：计划已定，这里只落盘——建落点、写登记记录。
/// 交回的里程碑是逐目标的身份与落点数，报告本身即这份清单
fn run_use(
    ctx: &mut Ctx<'_>,
    key: &str,
    plan: UsePlan,
) -> Result<Applied<UseOutcome, Vec<(Anchor, usize)>>, ProjectError> {
    let UsePlan {
        source,
        mut targets,
    } = plan;
    // 逐落点建入口；每建成一个就登记一条撤销，中途失败由骨架按逆序撤掉已建的入口。
    // 实际采用的链接方式逐落点记下：同一目标内的多个落点也可能落到不同档位
    let mut created: Vec<(PathBuf, LinkKind)> = Vec::new();
    for target in &mut targets {
        for dest in &target.destinations {
            match link::create_dir_link(&source, &dest.path) {
                Ok(kind) => {
                    target.link = Some(kind);
                    let undo = dest.path.clone();
                    ctx.on_rollback(move || {
                        // 回滚删不掉会留下半套入口，是唯一能发现残留的地方，必须留痕
                        if let Err(e) = link::remove_link(&undo, kind) {
                            tracing::warn!(path = %undo.display(), cause = %e, "回滚落点删除失败");
                        }
                    });
                    created.push((dest.path.clone(), kind));
                }
                Err(e) => {
                    // 建链接失败是本次使用的失败边界：记下哪个落点失败与原因，随后整批回滚
                    tracing::warn!(path = %dest.path.display(), cause = %e, "落点建链接失败");
                    return Err(e.into());
                }
            }
        }
    }
    // 一次使用的全部落点必须同一种链接方式：混用没法在一条登记记录里用一个值表达
    ensure_uniform_kinds(&created)?;
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
    commit_registry(ctx.paths, ctx.token, |registry| {
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
        outcome: project_outcome(key, false, targets, Vec::new()),
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
    actions: Vec<PlanStep>,
) -> UseOutcome {
    UseOutcome {
        dry_run,
        key: key.to_string(),
        actions,
        targets: targets
            .into_iter()
            .map(|target| TargetOutcome {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                link: target.link,
                destinations: target.destinations.into_iter().map(Into::into).collect(),
                actions: if dry_run { target.actions } else { Vec::new() },
            })
            .collect(),
    }
}
/// 全部落点必须采用同一种链接方式：登记记录只有一个 link 字段，
/// 混用没法用一条记录表达；不同档位的维护方式也不同（copy 不跟源同步）。
/// 判据是逐落点实际采用的档位，同一目标内的多个落点一并受检
pub(crate) fn ensure_uniform_kinds(created: &[(PathBuf, LinkKind)]) -> Result<(), ProjectError> {
    let Some((_, first)) = created.first() else {
        return Ok(());
    };
    if created.iter().all(|(_, kind)| kind == first) {
        return Ok(());
    }
    Err(ProjectError::MixedLinkKinds {
        kinds: created
            .iter()
            .map(|(_, kind)| format!("{kind:?}"))
            .collect(),
        paths: created
            .iter()
            .map(|(path, _)| path.display().to_string())
            .collect(),
    })
}
