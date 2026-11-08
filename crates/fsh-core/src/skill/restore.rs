//! 恢复：从最新版本 tag 把归档 skill 的编辑面取回，不打新 tag。

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SkillError, identity, lock_and_open, open_repo, workdir_exists};
use crate::git;
use crate::paths::HubPaths;

/// restore 的结果载荷：实跑直出 `{key, restored}`，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，实跑与预演两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 恢复的 skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 恢复完成标志（仅实跑出现，恒为 true）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub restored: bool,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}

/// 恢复：从最新版本 tag 把工作副本取回编辑面（元数据随内容树回来），
/// commit 一次，不打新 tag——版本链原样，只是编辑面复活。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn restore(hub_root: &Path, key: &str, dry_run: bool) -> Result<RestoreOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    if dry_run {
        let (repo, paths) = open_repo(hub_root)?;
        let plan = prepare_restore(&repo, &paths, key)?;
        return Ok(RestoreOutcome {
            dry_run: true,
            actions: plan.actions,
            ..RestoreOutcome::default()
        });
    }
    let started = std::time::Instant::now();
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_restore(&repo, &paths, key)?;
    let latest = plan.latest.as_str();
    let tag = crate::naming::version_tag(key, latest);
    // 编辑面：按 tag 树还原工作副本目录，frontmatter 全字段原样回来
    git::materialize(
        &repo,
        &tag,
        &crate::naming::store_subpath(key),
        &paths.skill_workdir(key),
    )?;
    let id = identity(&paths)?;
    git::commit_all(&repo, &format!("restore {key}"), &id.name, &id.email)?;
    // 恢复跨按 tag 树落盘与提交两次外部调用：键、取回版本、耗时记一条，便于对照版本链
    tracing::info!(
        key = %key,
        version = %latest,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "skill 恢复完成"
    );
    Ok(RestoreOutcome {
        key: key.into(),
        restored: true,
        ..RestoreOutcome::default()
    })
}

/// restore 预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct RestorePlan {
    /// 要取回的最新版本号
    latest: String,
    /// 将执行的动作清单
    actions: Vec<String>,
}

/// restore 预检：有版本 tag 才可恢复、工作副本在场则拒绝；动作清单在这里一次成形
fn prepare_restore(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<RestorePlan, SkillError> {
    let versions = git::tag_list(repo, key)?;
    if versions.is_empty() {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    if workdir_exists(paths, key) {
        return Err(SkillError::AlreadyExists { key: key.into() });
    }
    let latest = versions[0].clone();
    let actions = vec![
        format!(
            "checkout {} @ {}",
            crate::naming::store_subpath(key),
            crate::naming::version_tag(key, &latest)
        ),
        format!("commit: restore {key}"),
    ];
    Ok(RestorePlan { latest, actions })
}
