//! 发版：号由 frontmatter 定，fsh 校验后落章（commit + tag + 物化）；
//! 工作副本当前全部改动（含脏）一并收编进新版本。

use std::fs;
use std::path::Path;

use super::{
    BumpPart, SkillError, bump_semver, cmp_semver, invalid, lock_and_open, release,
    set_frontmatter_version, skill_identity, workdir_exists,
};
use crate::git;
use crate::paths::HubPaths;

/// 发版计划：目标号、当前最新版本与（--bump 时）需写回 frontmatter 的新文本
struct VersionPlan {
    /// 发布目标版本号
    target: String,
    /// 当前最新版本（无任何 tag 时为 None）
    current: Option<String>,
    /// --bump 代算出的新 frontmatter 文本；裸发版为 None（号本来就在文件里）
    rewrite: Option<String>,
}

/// 发版：读工作副本 frontmatter 的目标号（--bump 则按最新 tag 代算并写回
/// frontmatter），校验大于当前最新版本后 commit + tag + 物化。
/// 只对活跃 skill 开放；归档 skill 无编辑面可提交，报错。
pub fn version_bump(
    hub_root: &Path,
    key: &str,
    bump: Option<BumpPart>,
) -> Result<String, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_version(&repo, &paths, key, bump)?;
    // 代算的号先写回编辑面，再与其余改动一起收编进本次提交
    if let Some(text) = &plan.rewrite {
        fs::write(paths.skill_workdir(key).join("SKILL.md"), text)?;
    }
    release(
        &repo,
        &paths,
        key,
        &plan.target,
        &format!("release {key} v{}", plan.target),
        &_token,
    )?;
    Ok(plan.target)
}

/// 发版预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn bump_dry(
    hub_root: &Path,
    key: &str,
    bump: Option<BumpPart>,
) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_version(&repo, &paths, key, bump)?;
    let mut actions = Vec::new();
    if plan.rewrite.is_some() {
        actions.push(format!("写回 frontmatter version = {}", plan.target));
    }
    actions.push(format!("commit: release {key} v{}", plan.target));
    actions.push(format!(
        "tag {}",
        crate::naming::version_tag(key, &plan.target)
    ));
    actions.push(format!(
        "materialize {}",
        crate::naming::materialized_subpath(key, &plan.target)
    ));
    actions.push(format!(
        "stable 指针 {} -> {}",
        crate::naming::stable_ref(key),
        crate::naming::version_tag(key, &plan.target)
    ));
    if let Some(current) = &plan.current {
        actions.push(format!("版本 {current} -> {}", plan.target));
    }
    Ok(actions)
}

/// 发版预检：确认 skill 活跃、定出目标号并校验递增；执行与 dry-run 共用同一套判定
fn prepare_version(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    bump: Option<BumpPart>,
) -> Result<VersionPlan, SkillError> {
    let versions = git::tag_list(repo, key)?;
    if !workdir_exists(paths, key) {
        return Err(if versions.is_empty() {
            SkillError::SkillNotFound { key: key.into() }
        } else {
            SkillError::Archived { key: key.into() }
        });
    }
    let current = versions.first().cloned();
    let skill_md = paths.skill_workdir(key).join("SKILL.md");
    let label = skill_md.display().to_string();
    let text = fs::read_to_string(&skill_md)?;
    let fm = skill_identity(&text, &label)?;
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
    match bump {
        // 裸发版：目标号由文件定，只校验它确实向前走
        None => {
            let target = fm
                .version
                .ok_or_else(|| invalid(&label, "frontmatter 缺 version 字段（发版输入）".into()))?;
            if let Some(cur) = &current
                && cmp_semver(&target, cur) != std::cmp::Ordering::Greater
            {
                return Err(SkillError::VersionNotAdvancing {
                    key: key.into(),
                    current: cur.clone(),
                    target,
                });
            }
            Ok(VersionPlan {
                target,
                current,
                rewrite: None,
            })
        }
        // --bump 糖：按最新 tag 代算号，写回 frontmatter 后走同一套发版流程
        Some(part) => {
            let base = current.clone().unwrap_or_else(|| "0.0.0".into());
            let target = bump_semver(&base, part)?;
            let rewrite = set_frontmatter_version(&text, &target)?;
            Ok(VersionPlan {
                target,
                current,
                rewrite: Some(rewrite),
            })
        }
    }
}
