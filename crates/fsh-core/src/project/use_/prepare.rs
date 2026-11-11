//! 使用的预检：一次算清各目标铺到哪里、有没有冲突，产出可执行的计划。
//! 任一目标不合格即整体报错，实跑因此不会动盘；预演与实跑共用这一份计划。

use std::path::Path;

use super::{TargetPlan, UsePlan};
use crate::agents;
use crate::git;
use crate::hub;
use crate::identity::display_path;
use crate::paths::HubPaths;
use crate::project::Anchor;
use crate::project::ProjectError;
use crate::project::dest::{ensure_parent_is_dir, resolve_global, resolve_project};
use crate::project::target::dedupe_anchors;
use crate::skill::skill_status;

/// 共用预检：技能侧一次（存在性），再逐目标检查（登记冲突、落点空闲、父目录可用）。
/// `anchors` 里的项目目标是调用方给的原始写法，这里规范成绝对路径并去重
pub(super) fn prepare_use(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
    agents: &[String],
    anchors: Vec<Anchor>,
) -> Result<UsePlan, ProjectError> {
    // 状态由技能状态模块一次取全，这里的判定只剩「要什么」
    let status = skill_status(repo, paths, key)?;
    status.require_active()?;

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

    // 源只有一个：store 工作副本
    let source = paths.skill_workdir(key);

    // 登记簿与 agent 清单各读一次，逐目标复用
    let registry = hub::load_registry(paths)?;
    let known = agents::list(paths)?;
    let mut targets: Vec<TargetPlan> = Vec::new();
    for anchor in dedupe_anchors(anchors)? {
        // 登记闸门：该目标已有此技能的记录时拒绝，只新建、不覆盖已有记录
        match &anchor {
            Anchor::Project(dir) => {
                let already = registry
                    .project(dir)
                    .is_some_and(|p| p.skills.iter().any(|s| s.key == key));
                if already {
                    return Err(ProjectError::AlreadyRegistered {
                        key: key.into(),
                        project: display_path(dir),
                    });
                }
            }
            Anchor::Global => {
                if registry.global().skills.iter().any(|s| s.key == key) {
                    return Err(ProjectError::AlreadyRegisteredGlobal { key: key.into() });
                }
            }
        }

        let destinations = match &anchor {
            Anchor::Project(dir) => resolve_project(&known, dir, &agent_keys, key)?,
            Anchor::Global => resolve_global(&known, &agent_keys, key)?,
        };
        // 全部落点必须空闲；父目录链上有同名文件同样算占用
        for dest in &destinations {
            if dest.path.is_empty() || Path::new(&dest.path).symlink_metadata().is_ok() {
                return Err(ProjectError::DestinationOccupied {
                    path: dest.path.clone(),
                });
            }
            ensure_parent_is_dir(Path::new(&dest.path))?;
        }

        let mut actions = Vec::new();
        for dest in &destinations {
            actions.push(format!(
                "link {} -> {}（agents: {}）",
                source.display(),
                dest.path,
                dest.agents.join(", ")
            ));
        }
        actions.push(match &anchor {
            Anchor::Project(dir) => format!("register {} in {}", key, display_path(dir)),
            Anchor::Global => format!("register {key} in 全局"),
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
