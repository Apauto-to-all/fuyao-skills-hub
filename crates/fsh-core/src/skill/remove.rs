//! 两级删除：默认归档（删编辑面保版本链），purge 连版本 tag 一起删。

use std::fs;
use std::path::Path;

use super::{
    RemoveReport, SkillError, identity, lock_and_open, referencing_projects, skill_tags,
    workdir_exists,
};
use crate::git;
use crate::paths::HubPaths;

/// 两级删除：默认归档（删工作副本，版本 tag 与历史全留，供版不受影响）；
/// purge = true 时连全部版本 tag 一起删（历史 commit 保留，被项目引用则拒绝）。
pub fn remove(hub_root: &Path, key: &str, purge: bool) -> Result<RemoveReport, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let prep = prepare_remove(&repo, &paths, key, purge)?;

    let mut changed = false;
    if prep.had_workdir {
        fs::remove_dir_all(paths.skill_workdir(key))?;
        changed = true;
    }
    if changed {
        let id = identity(&paths)?;
        let verb = if purge { "purge" } else { "archive" };
        git::commit_all(&repo, &format!("{verb} {key}"), &id.name, &id.email)?;
    }
    if purge {
        for tag in &prep.purge_tags {
            repo.tag_delete(tag)?;
        }
    }
    Ok(RemoveReport {
        key: key.into(),
        purged: purge,
    })
}

/// remove 预检结果：执行与 dry-run 共用
struct RemovePrep {
    /// 工作副本在场（将被删）
    had_workdir: bool,
    /// purge 时将删的该键命名空间全部 tag（版本 + stable 指针）
    purge_tags: Vec<String>,
}

/// remove 预检：确认目标存在、purge 前查项目引用、收集将删 tag
fn prepare_remove(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    purge: bool,
) -> Result<RemovePrep, SkillError> {
    let versions = git::tag_list(repo, key)?;
    let had_workdir = workdir_exists(paths, key);
    if versions.is_empty() && !had_workdir {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    // 彻底删除前必须无人引用：账本说有项目在用就拒绝，不给强制开关
    if purge {
        let projects = referencing_projects(paths, key)?;
        if !projects.is_empty() {
            return Err(SkillError::Referenced {
                key: key.into(),
                projects,
            });
        }
    }
    let purge_tags = if purge {
        skill_tags(repo, key)?
    } else {
        Vec::new()
    };
    Ok(RemovePrep {
        had_workdir,
        purge_tags,
    })
}

/// remove 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn remove_dry(hub_root: &Path, key: &str, purge: bool) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let prep = prepare_remove(&repo, &paths, key, purge)?;
    let mut actions = Vec::new();
    if prep.had_workdir {
        actions.push(format!("remove {}", crate::naming::store_subpath(key)));
        let verb = if purge { "purge" } else { "archive" };
        actions.push(format!("commit: {verb} {key}"));
    }
    for tag in &prep.purge_tags {
        actions.push(format!("delete tag {tag}"));
    }
    Ok(actions)
}
