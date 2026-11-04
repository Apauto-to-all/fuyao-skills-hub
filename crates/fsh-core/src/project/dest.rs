//! 落点解析与落点守卫：agent 键 -> 实际落点、落点父目录检查、
//! 导出目录与 tag 一致性检查，以及项目侧的内容比对。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{Destination, ProjectError, path_identity};
use crate::agents;
use crate::paths::HubPaths;

/// 把 agent 键解析成实际落点：落点 = 项目根 + agent 技能目录 + 键末段，
/// 共用同一目录的 agent 合并成一条，保证一次使用只建一个入口
pub(super) fn resolve_destinations(
    paths: &HubPaths,
    project: &Path,
    agent_keys: &[String],
    key: &str,
) -> Result<Vec<Destination>, ProjectError> {
    let skill_name = key.rsplit('/').next().unwrap_or(key);
    let mut grouped: BTreeMap<String, Destination> = BTreeMap::new();
    for agent_key in agent_keys {
        let agent = agents::resolve(paths, agent_key)?;
        let dest = project.join(&agent.skills_dir).join(skill_name);
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
    repo: &git2::Repository,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    /// 任一侧读不了即报错，不静默判成「一致」——一致会被当成「可以安全撤回」
    #[test]
    fn dirs_equal_reports_error_when_a_side_is_unreadable() {
        let ok = tempfile::tempdir().unwrap();
        let gone = ok.path().join("no-such-subdir");
        let err = dirs_equal(ok.path(), &gone).unwrap_err();
        assert_eq!(err.error_code(), "io", "读不了目录应上抛 IO 错误");
    }

    /// 内容相同判一致，内容不同判不一致
    #[test]
    fn dirs_equal_compares_content() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        fs::write(a.path().join("x.txt"), "same").unwrap();
        fs::write(b.path().join("x.txt"), "same").unwrap();
        assert!(dirs_equal(a.path(), b.path()).unwrap());

        fs::write(b.path().join("x.txt"), "diff").unwrap();
        assert!(!dirs_equal(a.path(), b.path()).unwrap());
    }
}
