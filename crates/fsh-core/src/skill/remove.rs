//! 彻底删除：连全部版本 tag 一起删，被项目引用即拒绝，无强制开关。

use std::fs;
use std::path::Path;

use super::{
    SkillError, identity, lock_and_open, referencing_projects, skill_tags, workdir_exists,
};
use crate::git;
use crate::paths::HubPaths;

/// 彻底删除：先查登记簿（被引用即拒绝），再删工作副本、导出目录与
/// 该键命名空间下的全部版本 tag；历史 commit 保留。
pub fn remove(hub_root: &Path, key: &str) -> Result<(), SkillError> {
    let started = std::time::Instant::now();
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let tags = prepare_remove(&repo, &paths, key)?;
    if workdir_exists(&paths, key) {
        fs::remove_dir_all(paths.skill_workdir(key))?;
        let id = identity(&paths)?;
        git::commit_all(&repo, &format!("remove {key}"), &id.name, &id.email)?;
    }
    let export = paths.export().join(key);
    match fs::remove_dir_all(&export) {
        Ok(()) => {}
        // 导出目录不在是幂等命中而非故障：DEBUG 留痕，与归档路径同一口径
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(path = %export.display(), "导出目录不在，无需清理");
        }
        Err(e) => return Err(e.into()),
    }
    for tag in &tags {
        repo.tag_delete(tag)?;
    }
    // 移除不可逆：删掉的版本数与整段耗时一并落 INFO，事后可核对删了什么
    tracing::info!(
        key = %key,
        count = tags.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "skill 移除完成"
    );
    Ok(())
}

/// remove 预检：确认目标存在、无人引用；返回将删的该键命名空间全部 tag
fn prepare_remove(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<Vec<String>, SkillError> {
    let versions = git::tag_list(repo, key)?;
    if versions.is_empty() && !workdir_exists(paths, key) {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    // 彻底删除前必须无人引用：登记簿说有项目在用就拒绝，不给强制开关
    let projects = referencing_projects(paths, key)?;
    if !projects.is_empty() {
        return Err(SkillError::Referenced {
            key: key.into(),
            projects,
        });
    }
    skill_tags(repo, key)
}

/// remove 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn remove_dry(hub_root: &Path, key: &str) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let tags = prepare_remove(&repo, &paths, key)?;
    let mut actions = Vec::new();
    if workdir_exists(&paths, key) {
        actions.push(format!("remove {}", crate::naming::store_subpath(key)));
        actions.push(format!("commit: remove {key}"));
    }
    actions.push(format!(
        "remove {}/*",
        crate::naming::exported_key_subpath(key)
    ));
    for tag in &tags {
        actions.push(format!("delete tag {tag}"));
    }
    Ok(actions)
}
