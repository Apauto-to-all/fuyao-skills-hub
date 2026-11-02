//! 归档：删工作副本与导出目录，版本 tag 与 git 历史全留。
//! 与 remove 相对——归档可 restore，remove 连 tag 一起删。

use std::fs;
use std::path::Path;

use super::{SkillError, identity, lock_and_open, referencing_projects, workdir_exists};
use crate::git;
use crate::paths::HubPaths;

/// 归档：先查登记簿，被项目引用即拒绝；通过后删工作副本与导出目录，
/// tag 与历史全留，可 restore。
pub fn archive(hub_root: &Path, key: &str) -> Result<(), SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let had_workdir = prepare_archive(&repo, &paths, key)?;
    if had_workdir {
        fs::remove_dir_all(paths.skill_workdir(key))?;
        let id = identity(&paths)?;
        git::commit_all(&repo, &format!("archive {key}"), &id.name, &id.email)?;
    }
    remove_export_dirs(&paths, key)?;
    Ok(())
}

/// 归档预检：确认目标存在、且无人引用；返回工作副本是否在场
fn prepare_archive(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<bool, SkillError> {
    let versions = git::tag_list(repo, key)?;
    let had_workdir = workdir_exists(paths, key);
    if versions.is_empty() && !had_workdir {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    // 归档前必须无人引用：登记簿说有项目在用就拒绝，不给强制开关
    let projects = referencing_projects(paths, key)?;
    if !projects.is_empty() {
        return Err(SkillError::Referenced {
            key: key.into(),
            projects,
        });
    }
    Ok(had_workdir)
}

/// 删该键的全部导出目录；目录不在视为已清（幂等）
fn remove_export_dirs(paths: &HubPaths, key: &str) -> Result<(), SkillError> {
    match fs::remove_dir_all(paths.export().join(key)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// archive 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn archive_dry(hub_root: &Path, key: &str) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let had_workdir = prepare_archive(&repo, &paths, key)?;
    let mut actions = Vec::new();
    if had_workdir {
        actions.push(format!("remove {}", crate::naming::store_subpath(key)));
        actions.push(format!("commit: archive {key}"));
    }
    actions.push(format!(
        "remove {}/*",
        crate::naming::exported_key_subpath(key)
    ));
    Ok(actions)
}
