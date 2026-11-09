//! 彻底删除：连全部版本 tag 一起删，被项目引用即拒绝，无强制开关。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    SkillError, SkillState, ensure_unreferenced, identity, lock_and_open, open_repo, skill_status,
};
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

/// 彻底删除：先查登记簿（被引用即拒绝），再删工作副本、导出目录与
/// 该键命名空间下的全部版本 tag；历史 commit 保留。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn remove(hub_root: &Path, key: &str, dry_run: bool) -> Result<RemoveOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    if dry_run {
        let (repo, paths) = open_repo(hub_root)?;
        let plan = prepare_remove(&repo, &paths, key)?;
        return Ok(RemoveOutcome {
            dry_run: true,
            actions: plan.actions,
            ..RemoveOutcome::default()
        });
    }
    let started = std::time::Instant::now();
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_remove(&repo, &paths, key)?;
    if plan.had_workdir {
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
    for tag in &plan.tags {
        git::tag_delete(&repo, tag)?;
    }
    // 移除不可逆：删掉的版本数与整段耗时一并落 INFO，事后可核对删了什么
    tracing::info!(
        key = %key,
        count = plan.tags.len(),
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
    /// 将删的该键命名空间全部 tag
    tags: Vec<String>,
    /// 工作副本是否在场（决定要不要删掉并提交）
    had_workdir: bool,
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
    // 彻底删除前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    let tags = git::tag_keys(repo, key)?;
    let mut actions = Vec::new();
    if had_workdir {
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
    Ok(RemovePlan {
        tags,
        had_workdir,
        actions,
    })
}
