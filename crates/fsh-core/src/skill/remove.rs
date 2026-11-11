//! 彻底删除：删 store 工作副本，被项目引用即拒绝，无强制开关。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SkillError, SkillState, ensure_unreferenced, skill_status};
use crate::git;
use crate::paths::HubPaths;

/// remove 的结果载荷：实跑直出 `{key, removed}`，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，实跑与预演两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 移除的 skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 移除完成标志（仅实跑出现，恒为 true）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub removed: bool,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}

/// 彻底删除：先查登记簿（被引用即拒绝），再删 store 工作副本；
/// 历史 commit 留在审计日志里。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn remove(hub_root: &Path, key: &str, dry_run: bool) -> Result<RemoveOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    if dry_run {
        let paths = HubPaths::from_root(hub_root);
        let repo = git::open_hub(hub_root)?;
        let plan = prepare_remove(&repo, &paths, key)?;
        return Ok(RemoveOutcome {
            dry_run: true,
            actions: plan.actions,
            ..RemoveOutcome::default()
        });
    }
    let started = std::time::Instant::now();
    let (_token, repo, paths) = git::open_hub_for_write(hub_root)?;
    let plan = prepare_remove(&repo, &paths, key)?;
    if plan.had_workdir {
        fs::remove_dir_all(paths.skill_workdir(key))?;
        git::auto_commit(&repo, &format!("remove(store): {key}"));
    }
    if plan.had_archive {
        fs::remove_dir_all(paths.skill_archived(key))?;
        git::auto_commit(&repo, &format!("remove(store): {key}"));
    }
    // 移除不可逆：键与整段耗时一并落 INFO，事后可核对删了什么
    tracing::info!(
        key = %key,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "skill 移除完成"
    );
    Ok(RemoveOutcome {
        key: key.into(),
        removed: true,
        ..RemoveOutcome::default()
    })
}

/// remove 预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct RemovePlan {
    /// 工作副本是否在场（决定要不要删掉并提交）
    had_workdir: bool,
    /// 归档副本是否在场（决定要不要删掉并提交）
    had_archive: bool,
    /// 将执行的动作清单
    actions: Vec<String>,
}

/// remove 预检：确认目标存在、无人引用；动作清单在这里一次成形
fn prepare_remove(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<RemovePlan, SkillError> {
    let status = skill_status(repo, paths, key)?;
    status.require_present()?;
    let had_workdir = status.state() == SkillState::Active;
    let had_archive = status.state() == SkillState::Archived;
    // 彻底删除前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    let mut actions = Vec::new();
    if had_workdir {
        actions.push(format!("remove {}", crate::naming::store_subpath(key)));
        actions.push(format!("commit: remove(store): {key}"));
    }
    if had_archive {
        actions.push(format!("remove {}", crate::naming::archive_subpath(key)));
        actions.push(format!("commit: remove(store): {key}"));
    }
    Ok(RemovePlan {
        had_workdir,
        had_archive,
        actions,
    })
}
