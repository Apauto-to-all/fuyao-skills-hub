//! 发版：号由 frontmatter 定，fsh 校验后落章（commit + tag）；
//! 工作副本当前全部改动（含脏）一并收编进新版本。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::frontmatter::{parse, set_metadata_version};
use super::{
    SkillError, ensure_name_matches_dir, invalid, lock_and_open, open_repo, publish, workdir_exists,
};
use crate::git;
use crate::paths::HubPaths;
use crate::version::Version;

/// release 的结果载荷：实跑直出 `{key, version}`，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，实跑与预演两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 发版的 skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 落章的版本号（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub version: String,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}

/// 发版：读工作副本 frontmatter 的目标号，校验大于当前最新版本后
/// commit + tag。只对活跃 skill 开放；归档 skill 无编辑面可提交，报错。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn release(hub_root: &Path, key: &str, dry_run: bool) -> Result<ReleaseOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    if dry_run {
        let (repo, paths) = open_repo(hub_root)?;
        let plan = prepare_release(&repo, &paths, key)?;
        return Ok(ReleaseOutcome {
            dry_run: true,
            actions: plan.actions,
            ..ReleaseOutcome::default()
        });
    }
    let started = std::time::Instant::now();
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let target = prepare_release(&repo, &paths, key)?.target;
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
    Ok(ReleaseOutcome {
        key: key.into(),
        version: target,
        ..ReleaseOutcome::default()
    })
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
    // 键形状先判：界面输入同样过这一道，不靠前端自觉
    crate::key::validate(key)?;
    let started = std::time::Instant::now();
    let version = version.trim();
    // 号要原样写回 frontmatter 并进 tag：用与解析侧同一套三段判据，别造第二套
    if Version::parse(version).is_none() {
        return Err(SkillError::InvalidVersion {
            version: version.to_string(),
        });
    }
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let target = prepare_release_with(&repo, &paths, key, Some(version), overwrite)?.target;
    // 号在文件里是发版输入：写回后再提交，tag 指向的树与 frontmatter 一致
    let skill_md = paths.skill_workdir(key).join("SKILL.md");
    let text = fs::read_to_string(&skill_md)?;
    fs::write(&skill_md, set_metadata_version(&text, version)?)?;
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

/// release 预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct ReleasePlan {
    /// 目标版本号
    target: String,
    /// 将执行的动作清单
    actions: Vec<String>,
}

/// 发版预检：确认 skill 活跃、目标号来自 frontmatter 且确实向前走；
/// 执行与 dry-run 共用同一套判定
fn prepare_release(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<ReleasePlan, SkillError> {
    prepare_release_with(repo, paths, key, None, false)
}

/// 预检的共用实现：version 为 None 时从 frontmatter 取号（CLI 路径），
/// 为 Some 时以界面输入为准；overwrite 只放行与当前最新版本同号
fn prepare_release_with(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
    version: Option<&str>,
    overwrite: bool,
) -> Result<ReleasePlan, SkillError> {
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
    ensure_name_matches_dir(&fm, dir_name, &label)?;
    // 号来自界面输入或文件；文件缺号时只有界面能给
    let target = match version {
        Some(v) => v.to_string(),
        None => fm
            .version
            .ok_or_else(|| invalid(&label, "frontmatter 缺 metadata.version（发版输入）".into()))?,
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
    let actions = vec![
        format!("commit: release {key} v{target}"),
        format!("tag {}", crate::naming::version_tag(key, &target)),
    ];
    Ok(ReleasePlan { target, actions })
}
