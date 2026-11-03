//! 发版：号由 frontmatter 定，fsh 校验后落章（commit + tag）；
//! 工作副本当前全部改动（含脏）一并收编进新版本。

use std::fs;
use std::path::Path;

use super::{SkillError, cmp_semver, invalid, lock_and_open, parse, publish, workdir_exists};
use crate::git;
use crate::paths::HubPaths;

/// 发版：读工作副本 frontmatter 的目标号，校验大于当前最新版本后
/// commit + tag。只对活跃 skill 开放；归档 skill 无编辑面可提交，报错。
pub fn release(hub_root: &Path, key: &str) -> Result<String, SkillError> {
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let target = prepare_release(&repo, &paths, key)?;
    publish(
        &repo,
        &paths,
        key,
        &target,
        &format!("release {key} v{target}"),
        &token,
    )?;
    Ok(target)
}

/// 发版预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn release_dry(hub_root: &Path, key: &str) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let target = prepare_release(&repo, &paths, key)?;
    Ok(vec![
        format!("commit: release {key} v{target}"),
        format!("tag {}", crate::naming::version_tag(key, &target)),
    ])
}

/// 发版预检：确认 skill 活跃、目标号来自 frontmatter 且确实向前走；
/// 执行与 dry-run 共用同一套判定
fn prepare_release(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<String, SkillError> {
    let versions = git::tag_list(repo, key)?;
    if !workdir_exists(paths, key) {
        return Err(if versions.is_empty() {
            SkillError::SkillNotFound { key: key.into() }
        } else {
            SkillError::Archived { key: key.into() }
        });
    }
    let skill_md = paths.skill_workdir(key).join("SKILL.md");
    let label = skill_md.display().to_string();
    let text = fs::read_to_string(&skill_md)?;
    let fm = parse(&text, &label)?;
    // 发版时同样守住身份约束：name = 键末段（= SKILL.md 所在目录名）
    let dir_name = key.rsplit('/').next().unwrap_or(key);
    if fm.name != dir_name {
        return Err(invalid(
            &label,
            format!(
                "frontmatter name ({}) 与所在目录名 ({dir_name}) 不一致",
                fm.name
            ),
        ));
    }
    // 号由文件定，只校验它确实向前走
    let target = fm
        .version
        .ok_or_else(|| invalid(&label, "frontmatter 缺 version 字段（发版输入）".into()))?;
    if let Some(cur) = versions.first()
        && cmp_semver(&target, cur) != std::cmp::Ordering::Greater
    {
        return Err(SkillError::VersionNotAdvancing {
            key: key.into(),
            current: cur.clone(),
            target,
        });
    }
    Ok(target)
}
