//! 落点解析与落点守卫：agent 键 -> 实际落点、落点父目录检查、
//! 导出目录与 tag 一致性检查，以及项目侧的内容比对。

use crate::git;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{Destination, ProjectError};
use crate::agents::{self, AgentInfo};
use crate::identity::path_identity;
use crate::paths::HubPaths;

/// 把 agent 键解析成实际落点：落点 = 项目根 + agent 技能目录 + 键末段，
/// 共用同一目录的 agent 合并成一条，保证一次使用只建一个入口
pub(super) fn resolve_destinations(
    paths: &HubPaths,
    project: &Path,
    agent_keys: &[String],
    key: &str,
) -> Result<Vec<Destination>, ProjectError> {
    let known = agents::load_agents(paths)?;
    resolve_from(&known, project, agent_keys, key)
}

/// 用已取回的 agent 清单解析落点：整页只扫一次 agent 目录，不逐 agent 重读
pub(super) fn resolve_from(
    known: &[AgentInfo],
    project: &Path,
    agent_keys: &[String],
    key: &str,
) -> Result<Vec<Destination>, ProjectError> {
    let skill_name = key.rsplit('/').next().unwrap_or(key);
    let mut grouped: BTreeMap<String, Destination> = BTreeMap::new();
    for agent_key in agent_keys {
        let agent = known
            .iter()
            .find(|agent| agent.key == *agent_key)
            .ok_or_else(|| agents::AgentError::UnknownAgent {
                key: agent_key.clone(),
                known: known
                    .iter()
                    .map(|agent| agent.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            })?;
        // 项目落点要用 agent 的项目技能目录；只有全局位的 agent 到此为止
        let skills_dir =
            agent
                .skills_dir
                .as_deref()
                .ok_or_else(|| agents::AgentError::NoProjectDir {
                    key: agent.key.clone(),
                })?;
        let dest = project.join(skills_dir).join(skill_name);
        grouped
            .entry(path_identity(&dest))
            .or_insert_with(|| Destination {
                path: dest.display().to_string(),
                agents: Vec::new(),
            })
            .agents
            .push(agent_key.clone());
    }
    let destinations: Vec<Destination> = grouped.into_values().collect();
    // 目标路径计算是落点的唯一来源：DEBUG 记下项目与落点数，核对合并去重结果
    tracing::debug!(
        path = %project.display(),
        count = destinations.len(),
        "目标路径已解析"
    );
    Ok(destinations)
}

/// 落点父目录链上最近的存在项必须是目录：否则建链接会在中途失败
pub(super) fn ensure_parent_is_dir(dest: &Path, project: &Path) -> Result<(), ProjectError> {
    let mut cur = dest.parent();
    while let Some(dir) = cur {
        if dir.symlink_metadata().is_ok() {
            if dir.is_dir() {
                return Ok(());
            }
            return Err(ProjectError::DestinationOccupied {
                path: dir.display().to_string(),
            });
        }
        if path_identity(dir) == path_identity(project) {
            break;
        }
        cur = dir.parent();
    }
    Ok(())
}

/// 已有导出目录必须先与 tag 一致才复用；不一致保留现场并报错。
/// 导出目录读不出来时也算不一致——读不出来就证明不了一致，按不能复用处理
pub(super) fn ensure_export_matches(
    repo: &git::Repository,
    key: &str,
    version: &str,
    dir: &Path,
) -> Result<(), ProjectError> {
    let tag = crate::naming::version_tag(key, version);
    let expected = crate::fingerprint::tag_digest(repo, &tag, key);
    let actual = crate::fingerprint::dir_digest_best_effort(dir);
    if expected == actual {
        // 一致即复用现成导出目录：这是幂等命中，DEBUG 记下被复用的目录
        tracing::debug!(path = %dir.display(), "导出目录已复用");
        Ok(())
    } else {
        Err(ProjectError::DirtyExport {
            path: dir.display().to_string(),
        })
    }
}

/// 落点实际形态的人读描述（报错用）
pub(super) fn describe_kind(meta: &fs::Metadata) -> String {
    if meta.file_type().is_symlink() {
        "符号链接".to_string()
    } else if meta.is_dir() {
        "普通目录".to_string()
    } else {
        "普通文件".to_string()
    }
}

/// 两个目录内容是否一致：两侧各算一把同口径指纹再比。
/// 任一侧读不了即报错，由调用方据此判断，不静默当作一致
pub(super) fn dirs_equal(a: &Path, b: &Path) -> Result<bool, ProjectError> {
    Ok(crate::fingerprint::dir_digest(a)? == crate::fingerprint::dir_digest(b)?)
}

/// 某键在登记簿里已接入的落点数：逐项目解析 agent 目录，共用目录的 agent 计一
pub(crate) fn destination_count(
    paths: &HubPaths,
    registry: &crate::hub::ProjectRegistry,
    key: &str,
) -> Result<usize, ProjectError> {
    let known = agents::load_agents(paths)?;
    let mut count = 0;
    for (path, project) in &registry.projects {
        let Some(skill) = project.skills.iter().find(|s| s.key == key) else {
            continue;
        };
        count += resolve_from(&known, Path::new(path), &skill.agents, key)?.len();
    }
    Ok(count)
}
