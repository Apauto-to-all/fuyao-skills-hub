//! 发版：号由 frontmatter 定，fsh 校验后落章（commit + tag）；
//! 工作副本当前全部改动（含脏）一并收编进新版本。

use std::fs;
use std::path::Path;

use super::frontmatter::{parse, set_frontmatter_version};
use super::{SkillError, invalid, lock_and_open, publish, workdir_exists};
use crate::git;
use crate::paths::HubPaths;
use crate::version::Version;

/// 发版：读工作副本 frontmatter 的目标号，校验大于当前最新版本后
/// commit + tag。只对活跃 skill 开放；归档 skill 无编辑面可提交，报错。
pub fn release(hub_root: &Path, key: &str) -> Result<String, SkillError> {
    let started = std::time::Instant::now();
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
    // 发版是版本链唯一的推进点：提交与打标两段外部调用的耗时一起记，慢库可据此定位
    tracing::info!(
        key = %key,
        version = %target,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "版本发布完成"
    );
    Ok(target)
}

/// 按界面输入的版本号发版：先把号写回工作副本 frontmatter，再走同一套发版。
/// overwrite 只放行「等于当前最新版本」的同号重发（tag 强制移到新提交）；
/// 更旧的号一律拒绝——已发布版本是项目引用的来源，不能悄悄改写。
pub fn release_version(
    hub_root: &Path,
    key: &str,
    version: &str,
    overwrite: bool,
) -> Result<String, SkillError> {
    let started = std::time::Instant::now();
    let version = version.trim();
    // 号要原样写回 frontmatter 并进 tag：用与解析侧同一套三段判据，别造第二套
    if Version::parse(version).is_none() {
        return Err(SkillError::InvalidVersion {
            version: version.to_string(),
        });
    }
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let target = prepare_release_with(&repo, &paths, key, Some(version), overwrite)?;
    // 号在文件里是发版输入：写回后再提交，tag 指向的树与 frontmatter 一致
    let skill_md = paths.skill_workdir(key).join("SKILL.md");
    let text = fs::read_to_string(&skill_md)?;
    fs::write(&skill_md, set_frontmatter_version(&text, version)?)?;
    publish(
        &repo,
        &paths,
        key,
        &target,
        &format!("release {key} v{target}"),
        &token,
    )?;
    tracing::info!(
        key = %key,
        version = %target,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "版本发布完成"
    );
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
    prepare_release_with(repo, paths, key, None, false)
}

/// 预检的共用实现：version 为 None 时从 frontmatter 取号（CLI 路径），
/// 为 Some 时以界面输入为准；overwrite 只放行与当前最新版本同号
fn prepare_release_with(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    version: Option<&str>,
    overwrite: bool,
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
    // 号来自界面输入或文件；文件缺号时只有界面能给
    let target = match version {
        Some(v) => v.to_string(),
        None => fm
            .version
            .ok_or_else(|| invalid(&label, "frontmatter 缺 version 字段（发版输入）".into()))?,
    };
    if let Some(cur) = versions.first() {
        // 目标号刚校验过；`cur` 来自 tag 反解，两侧任一解析不出就无法比较，按「不许前进」处理
        let ordered = match (Version::parse(&target), Version::parse(cur)) {
            (Some(next), Some(current)) => next.cmp(&current),
            _ => std::cmp::Ordering::Less,
        };
        match ordered {
            std::cmp::Ordering::Greater => {}
            // 同号重发须显式覆盖：tag 会被强制移到新提交
            std::cmp::Ordering::Equal if overwrite => {}
            _ => {
                return Err(SkillError::VersionNotAdvancing {
                    key: key.into(),
                    current: cur.clone(),
                    target,
                });
            }
        }
    }
    Ok(target)
}
