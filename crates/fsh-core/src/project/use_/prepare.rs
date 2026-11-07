//! 使用的预检：一次算清各目标铺到哪里、有没有冲突，产出可执行的计划。
//! 任一目标不合格即整体报错，实跑因此不会动盘；预演与实跑共用这一份计划。

use std::path::Path;

use super::{Anchor, TargetPlan, UsePlan};
use crate::agents;
use crate::git;
use crate::hub;
use crate::identity::{display_path, path_identity};
use crate::paths::HubPaths;
use crate::project::dest::{
    ensure_export_matches, ensure_parent_is_dir, resolve_global, resolve_project,
};
use crate::project::{ProjectError, canonical_project};

/// 共用预检：技能侧一次（存在性、归档、版本、导出目录），
/// 再逐目标检查（登记冲突、落点空闲、父目录可用）。
/// `anchors` 里的项目目标是调用方给的原始写法，这里规范成绝对路径并去重
pub(super) fn prepare_use(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
    version: Option<&str>,
    agents: &[String],
    anchors: Vec<Anchor>,
) -> Result<UsePlan, ProjectError> {
    let versions = git::tag_list(repo, key)?;
    let workdir = paths.skill_workdir(key);
    if !workdir.is_dir() {
        if versions.is_empty() {
            return Err(ProjectError::SkillNotFound { key: key.into() });
        }
        return Err(ProjectError::Archived { key: key.into() });
    }
    if let Some(v) = version
        && !versions.iter().any(|have| have == v)
    {
        return Err(ProjectError::VersionNotFound {
            key: key.into(),
            version: v.into(),
        });
    }

    let mut agent_keys: Vec<String> = Vec::new();
    for a in agents {
        if !agent_keys.iter().any(|e| e == a) {
            agent_keys.push(a.clone());
        }
    }
    if agent_keys.is_empty() {
        return Err(ProjectError::InvalidSkillSpec {
            spec: String::new(),
        });
    }

    let (source, export_dir, version_owned) = match version {
        Some(v) => {
            let dir = paths.exported(key, v);
            if dir.is_dir() {
                ensure_export_matches(repo, key, v, &dir)?;
            }
            (dir.clone(), Some(dir), Some(v.to_string()))
        }
        None => (workdir.clone(), None, None),
    };

    // 登记簿与 agent 清单各读一次，逐目标复用
    let registry = hub::load_registry(paths)?;
    let known = agents::load_agents(paths)?;
    let mut targets: Vec<TargetPlan> = Vec::new();
    let mut seen: Vec<std::path::PathBuf> = Vec::new();
    for anchor in anchors {
        let anchor = match anchor {
            Anchor::Global => Anchor::Global,
            Anchor::Project(dir) => {
                let dir = canonical_project(&dir)?;
                // 同一目录写两遍是同一个目标：只服务一次
                if seen
                    .iter()
                    .any(|seen| path_identity(seen) == path_identity(&dir))
                {
                    tracing::debug!(project = %dir.display(), "重复项目目标跳过");
                    continue;
                }
                seen.push(dir.clone());
                Anchor::Project(dir)
            }
        };

        // 登记闸门：该目标已有此技能的记录时拒绝（升级 / 回滚先 unuse）
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
    if let Some(dir) = export_dir.as_ref().filter(|d| !d.is_dir()) {
        let line = format!(
            "export {} -> {}",
            version_owned.as_deref().unwrap_or(""),
            dir.display()
        );
        for target in &mut targets {
            target.actions.insert(0, line.clone());
        }
    }

    Ok(UsePlan {
        source,
        version: version_owned,
        export_dir,
        targets,
    })
}
