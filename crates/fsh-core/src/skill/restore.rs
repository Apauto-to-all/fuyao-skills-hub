//! 恢复：从最新版本 tag 把归档 skill 的编辑面取回，不打新 tag。

use std::fs;
use std::path::Path;

use super::{SkillError, identity, lock_and_open, meta_path, workdir_exists};
use crate::git;
use crate::paths::HubPaths;

/// 恢复：从最新版本 tag 把工作副本与 meta 取回编辑面，commit 一次，
/// 不打新 tag——版本链原样，只是编辑面复活。
pub fn restore(hub_root: &Path, skill: &str) -> Result<(), SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let latest = prepare_restore(&repo, &paths, skill)?;
    let tag = crate::naming::version_tag(skill, &latest);
    // 编辑面：物化路径直接写回工作副本目录
    git::materialize(
        &repo,
        &tag,
        &crate::naming::store_subpath(skill),
        &paths.skill_workdir(skill),
    )?;
    // meta：从 tag 内容树取 blob 写回
    if let Ok((_, tree_id)) = git::resolve(&repo, &tag) {
        let tree = repo.find_tree(tree_id)?;
        if let Ok(entry) = tree.get_path(Path::new(&format!("meta/{skill}.toml"))) {
            let obj = entry.to_object(&repo)?;
            let blob = obj.peel_to_blob()?;
            fs::create_dir_all(paths.meta())?;
            fs::write(meta_path(&paths, skill), blob.content())?;
        }
    }
    let id = identity(&paths)?;
    git::commit_all(&repo, &format!("restore {skill}"), &id.name, &id.email)?;
    Ok(())
}

/// restore 预检：有版本 tag 才可恢复、工作副本在场则拒绝；返回最新版本号。
/// 执行与 dry-run 共用同一套判定
fn prepare_restore(
    repo: &git2::Repository,
    paths: &HubPaths,
    skill: &str,
) -> Result<String, SkillError> {
    let versions = git::tag_list(repo, skill)?;
    if versions.is_empty() {
        return Err(SkillError::SkillNotFound {
            skill: skill.into(),
        });
    }
    if workdir_exists(paths, skill) {
        return Err(SkillError::AlreadyExists {
            skill: skill.into(),
        });
    }
    Ok(versions[0].clone())
}

/// restore 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn restore_dry(hub_root: &Path, skill: &str) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let latest = prepare_restore(&repo, &paths, skill)?;
    Ok(vec![
        format!(
            "checkout {} @ {}",
            crate::naming::store_subpath(skill),
            crate::naming::version_tag(skill, &latest)
        ),
        format!("restore meta/{skill}.toml"),
        format!("commit: restore {skill}"),
    ])
}
