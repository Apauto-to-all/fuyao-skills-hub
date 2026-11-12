//! 使用的执行：按计划建落点、写登记记录。
//! 任何一步失败都把本次已建落点删净——盘上不留没有记录的入口，登记簿里也不留没有入口的记录。

use std::path::{Path, PathBuf};

use super::prepare::prepare_use;
use super::{TargetPlan, UsePlan};
use crate::commit::Verb;
use crate::link::{self, LinkKind};
use crate::project::ledger::{commit_registry, usage_message};
use crate::project::target::anchors_or_reject;
use crate::project::{Anchor, ProjectError, TargetOutcome, Targets, UseOutcome};
use crate::write::{self, Applied, Ctx, OnFailure};

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
        |plan| project_outcome(key, true, plan.targets),
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

    // 登记簿：各目标一次写入，项目目标 upsert + 技能记录，全局目标进全局段；
    // 预检已把项目根规范成绝对路径，登记簿与落点因此共用同一身份
    commit_registry(ctx.paths, ctx.token, |registry| {
        for target in &targets {
            let Some(kind) = target.link else { continue };
            let record = crate::hub::ProjectSkill {
                key: key.to_string(),
                agents: target.agents.clone(),
                link: kind,
            };
            match &target.anchor {
                Anchor::Project(dir) => {
                    registry.upsert(dir, crate::hub::unix_now());
                    // upsert 刚从身份键建过这条记录，取不到只可能是登记簿内部不一致
                    let Some(entry) = registry.project_mut(dir) else {
                        return Err(ProjectError::InvalidProject {
                            path: dir.display().to_string(),
                        });
                    };
                    entry.skills.push(record);
                }
                Anchor::Global => registry.global_mut().skills.push(record),
            }
        }
        Ok(())
    })?;

    // 落盘成功后的机器提交：消息只用 agent 名、键名与项目名等相对标识
    let agents = targets
        .first()
        .map(|target| target.agents.clone())
        .unwrap_or_default();
    let anchors: Vec<&Anchor> = targets.iter().map(|target| &target.anchor).collect();
    let commits = vec![usage_message(Verb::Use, key, &agents, &anchors)];
    let milestones: Vec<(Anchor, usize)> = targets
        .iter()
        .map(|target| (target.anchor.clone(), target.destinations.len()))
        .collect();
    Ok(Applied {
        outcome: project_outcome(key, false, targets),
        commits,
        milestones,
    })
}

/// 结果投影：预演与实跑共用这一处。计划给出身份、agent、落点与动作清单，
/// 两者的差别只有一条：链接方式要真建入口才有值，因此实跑带 `link`、不带动作清单，
/// 预演反之。载荷加字段只改这里
fn project_outcome(key: &str, dry_run: bool, targets: Vec<TargetPlan>) -> UseOutcome {
    UseOutcome {
        dry_run,
        key: key.to_string(),
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
