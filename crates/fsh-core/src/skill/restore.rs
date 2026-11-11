//! 恢复：把归档目录里的键整目录移回 store；store 里同名键已占用即报冲突，
//! 不做静默改名。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SkillError, SkillState, skill_status};
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

/// 恢复：把归档目录里的键移回 store（store 里同名键已占用报 conflict），
/// 并自动提交。dry_run = true 时预演：只读不落盘、不取写锁。
pub fn restore(hub_root: &Path, key: &str, dry_run: bool) -> Result<RestoreOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    if dry_run {
        let paths = HubPaths::from_root(hub_root);
        let repo = git::open_hub(hub_root)?;
        let actions = prepare_restore(&repo, &paths, key)?;
        return Ok(RestoreOutcome {
            dry_run: true,
            actions,
            ..RestoreOutcome::default()
        });
    }
    let started = std::time::Instant::now();
    let (_token, repo, paths) = git::open_hub_for_write(hub_root)?;
    prepare_restore(&repo, &paths, key)?;
    let src = paths.skill_archived(key);
    let dst = paths.skill_workdir(key);
    fs::rename(&src, &dst)?;
    git::auto_commit(&repo, &format!("restore(store): {key}"));
    // 恢复跨移动目录与机器提交两段外部操作：一条 INFO 交代键与整段耗时
    tracing::info!(
        key = %key,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "skill 恢复完成"
    );
    Ok(RestoreOutcome {
        key: key.into(),
        restored: true,
        ..RestoreOutcome::default()
    })
}

/// 恢复预检：归档副本必须在场、store 里同名键必须空闲；动作清单在这里一次成形
fn prepare_restore(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<Vec<String>, SkillError> {
    let status = skill_status(repo, paths, key)?;
    status.require_present()?;
    if status.state() != SkillState::Archived {
        // store 里已有同键工作副本：与归档侧占用同款冲突
        return Err(SkillError::Conflict { key: key.into() });
    }
    Ok(vec![
        format!(
            "move {} -> {}",
            crate::naming::archive_subpath(key),
            crate::naming::store_subpath(key)
        ),
        format!("commit: restore(store): {key}"),
    ])
}
