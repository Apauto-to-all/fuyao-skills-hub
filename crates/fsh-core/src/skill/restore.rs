//! 恢复：从最新版本 tag 把归档 skill 的编辑面取回，不打新 tag。

use std::path::Path;

use super::{SkillError, identity, lock_and_open, workdir_exists};
use crate::git;
use crate::paths::HubPaths;

/// 恢复：从最新版本 tag 把工作副本取回编辑面（元数据随内容树回来），
/// commit 一次，不打新 tag——版本链原样，只是编辑面复活。
pub fn restore(hub_root: &Path, key: &str) -> Result<(), SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let latest = prepare_restore(&repo, &paths, key)?;
    let tag = crate::naming::version_tag(key, &latest);
    // 编辑面：物化路径直接写回工作副本目录，frontmatter 全字段原样回来
    git::materialize(
        &repo,
        &tag,
        &crate::naming::store_subpath(key),
        &paths.skill_workdir(key),
    )?;
    let id = identity(&paths)?;
    git::commit_all(&repo, &format!("restore {key}"), &id.name, &id.email)?;
    Ok(())
}

/// restore 预检：有版本 tag 才可恢复、工作副本在场则拒绝；返回最新版本号。
/// 执行与 dry-run 共用同一套判定
fn prepare_restore(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<String, SkillError> {
    let versions = git::tag_list(repo, key)?;
    if versions.is_empty() {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    if workdir_exists(paths, key) {
        return Err(SkillError::AlreadyExists { key: key.into() });
    }
    Ok(versions[0].clone())
}

/// restore 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn restore_dry(hub_root: &Path, key: &str) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let latest = prepare_restore(&repo, &paths, key)?;
    Ok(vec![
        format!(
            "checkout {} @ {}",
            crate::naming::store_subpath(key),
            crate::naming::version_tag(key, &latest)
        ),
        format!("commit: restore {key}"),
    ])
}
