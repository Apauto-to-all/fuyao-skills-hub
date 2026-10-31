//! 发版：把工作副本当前全部改动（含脏）打成新版本。

use std::path::Path;

use super::{BumpPart, SkillError, bump_semver, lock_and_open, release, workdir_exists};
use crate::git;
use crate::paths::HubPaths;

/// 发版：把工作副本当前全部改动（含脏）打成新版本——commit + tag + 物化。
/// 只对活跃 skill 开放；归档 skill 无编辑面可提交，报错。
pub fn version_bump(hub_root: &Path, skill: &str, part: BumpPart) -> Result<String, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let (_current, next) = prepare_bump(&repo, &paths, skill, part)?;
    release(
        &repo,
        &paths,
        skill,
        &next,
        &format!("release {skill} v{next}"),
    )?;
    Ok(next)
}

/// 发版预检：确认 skill 活跃、算出下一版号；执行与 dry-run 共用同一套判定
fn prepare_bump(
    repo: &git2::Repository,
    paths: &HubPaths,
    skill: &str,
    part: BumpPart,
) -> Result<(String, String), SkillError> {
    let versions = git::tag_list(repo, skill)?;
    if !workdir_exists(paths, skill) {
        return Err(if versions.is_empty() {
            SkillError::SkillNotFound {
                skill: skill.into(),
            }
        } else {
            SkillError::Archived {
                skill: skill.into(),
            }
        });
    }
    let current = versions.first().ok_or(SkillError::SkillNotFound {
        skill: skill.into(),
    })?;
    let next = bump_semver(current, part)?;
    Ok((current.clone(), next))
}

/// 发版预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn bump_dry(hub_root: &Path, skill: &str, part: BumpPart) -> Result<Vec<String>, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let (current, next) = prepare_bump(&repo, &paths, skill, part)?;
    Ok(vec![
        format!("commit: release {skill} v{next}"),
        format!("tag {skill}/v{next}"),
        format!("materialize store/{skill}/{next}"),
        format!("stable 指针 {skill}/stable -> {skill}/v{next}"),
        format!("版本 {current} -> {next}"),
    ])
}
