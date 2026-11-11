//! 使用的执行：按计划建落点、写登记记录。
//! 任何一步失败都把本次已建落点删净——盘上不留没有记录的入口，登记簿里也不留没有入口的记录。

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::UsePlan;
use super::prepare::prepare_use;
use crate::git;
use crate::hub;
use crate::link::{self, LinkKind};
use crate::lock::WriteToken;
use crate::paths::HubPaths;
use crate::project::skeleton::{
    Lead, Milestone, Session, commit_message, commit_registry, lead, log_milestones,
};
use crate::project::{Anchor, ProjectError, TargetOutcome, Targets, UseOutcome, rollback_links};

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
    // 相同前导（键校验、目标解析、无目标拒绝、预演分叉、取锁）两入口共用
    let Lead { anchors, session } = lead(hub_root, key, &targets, dry_run)?;
    match session {
        Session::Dry { repo, paths } => {
            let plan = prepare_use(&repo, &paths, key, agents, anchors)?;
            Ok(planned_outcome(key, plan))
        }
        Session::Write { token, repo, paths } => {
            let plan = prepare_use(&repo, &paths, key, agents, anchors)?;
            run_use(&token, &repo, &paths, key, plan)
        }
    }
}

/// 已持锁的执行段：计划已定，这里只落盘——建落点、写登记记录
fn run_use(
    token: &WriteToken,
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
    plan: UsePlan,
) -> Result<UseOutcome, ProjectError> {
    // 这次使用跨建链接、写登记簿、机器提交三次外部交互，用总耗时回答「慢在哪」
    let started = Instant::now();
    let UsePlan {
        source,
        mut targets,
    } = plan;

    // 逐落点建入口；任一步失败即回滚本次已建的全部入口
    let mut created: Vec<(PathBuf, LinkKind)> = Vec::new();
    for target in &mut targets {
        for dest in &target.destinations {
            match link::create_dir_link(&source, Path::new(&dest.path)) {
                Ok(kind) => {
                    created.push((PathBuf::from(&dest.path), kind));
                    target.link = Some(kind);
                }
                Err(e) => {
                    // 建链接失败是本次使用的失败边界：记下哪个落点失败与原因，随后整批回滚
                    tracing::warn!(path = %dest.path, cause = %e, "落点建链接失败");
                    rollback_links(&created);
                    return Err(e.into());
                }
            }
        }
    }

    // 一次使用的全部落点必须同一种链接方式：混用没法在一条登记记录里用一个值表达
    let mut kinds = created.iter().map(|(_, kind)| *kind);
    if let Some(first) = kinds.next()
        && kinds.any(|kind| kind != first)
    {
        tracing::warn!(count = created.len(), "落点链接方式混用失败");
        let kinds = created
            .iter()
            .map(|(_, kind)| format!("{kind:?}"))
            .collect();
        let paths = created
            .iter()
            .map(|(path, _)| path.display().to_string())
            .collect();
        rollback_links(&created);
        return Err(ProjectError::MixedLinkKinds { kinds, paths });
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

    // 登记簿：各目标一次写入，项目目标 upsert + 技能记录，全局目标进全局段；
    // 预检已把项目根规范成绝对路径，登记簿与落点因此共用同一身份。
    // 读不出、写不进登记簿同样回滚：否则留下没有记录的落点
    commit_registry(
        paths,
        token,
        || rollback_links(&created),
        |registry| {
            for target in &targets {
                let Some(kind) = target.link else { continue };
                let record = hub::ProjectSkill {
                    key: key.to_string(),
                    agents: target.agents.clone(),
                    link: kind,
                };
                match &target.anchor {
                    Anchor::Project(dir) => {
                        registry.upsert(dir, hub::unix_now());
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
        },
    )?;

    // 落盘成功后的机器提交：消息只用 agent 名、键名与项目名等相对标识
    let agents = targets
        .first()
        .map(|t| t.agents.clone())
        .unwrap_or_default();
    let anchors: Vec<&Anchor> = targets.iter().map(|t| &t.anchor).collect();
    git::auto_commit(repo, &commit_message("use", key, &agents, &anchors));

    // 使用完成是业务里程碑：逐目标各记一条，读日志即可重建「哪个目标铺了什么」
    log_milestones(
        key,
        started,
        targets.iter().map(|target| Milestone {
            anchor: &target.anchor,
            count: target.destinations.len(),
            done: "使用完成",
            skipped: None,
        }),
    );
    Ok(UseOutcome {
        dry_run: false,
        key: key.to_string(),
        targets: targets
            .into_iter()
            .map(|target| TargetOutcome {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                link: target.link,
                destinations: target.destinations,
                actions: Vec::new(),
            })
            .collect(),
    })
}

/// 预演结果：各目标的身份、agent、计划落点与动作清单齐备；
/// 链接方式要真建才知道档位，预演因此不给
fn planned_outcome(key: &str, plan: UsePlan) -> UseOutcome {
    UseOutcome {
        dry_run: true,
        key: key.to_string(),
        targets: plan
            .targets
            .into_iter()
            .map(|target| TargetOutcome {
                scope: target.anchor.scope(),
                project: target.anchor.project(),
                agents: target.agents,
                destinations: target.destinations,
                actions: target.actions,
                ..TargetOutcome::default()
            })
            .collect(),
    }
}
