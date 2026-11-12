//! 彻底删除：两侧副本（store 工作副本与 archive 归档副本）哪侧在场删哪侧，
//! 被项目引用即拒绝，无强制开关。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{SkillError, ensure_unreferenced, skill_occupancy};
use crate::commit::{Face, Verb, message};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep};
use crate::write::{self, Applied, OnFailure};

/// 预演载荷：预演标志置真，动作清单即计划里的步骤
impl From<Vec<PlanStep>> for RemoveOutcome {
    fn from(actions: Vec<PlanStep>) -> Self {
        Self {
            dry_run: true,
            actions,
            ..Self::default()
        }
    }
}

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
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}

/// 彻底删除：先查登记簿（被引用即拒绝），再删 store 工作副本；
/// 历史 commit 留在审计日志里。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn remove(hub_root: &Path, key: &str, dry_run: bool) -> Result<RemoveOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    write::run(
        hub_root,
        dry_run,
        // 删除不回滚：删到一半失败时已删的部分保持删掉，重跑即可续完
        OnFailure::Resume,
        |paths| prepare_remove(paths, key),
        |plan| RemoveOutcome::from(plan.actions),
        |_ctx, plan| {
            let mut commits = Vec::new();
            if let Some(dir) = &plan.workdir {
                fs::remove_dir_all(dir)?;
                commits.push(message(Verb::Remove, Face::Store, key));
            }
            if let Some(dir) = &plan.archive {
                fs::remove_dir_all(dir)?;
                commits.push(message(Verb::Remove, Face::Archive, key));
            }
            Ok(Applied {
                outcome: RemoveOutcome {
                    key: key.into(),
                    removed: true,
                    ..RemoveOutcome::default()
                },
                commits,
                milestones: (),
            })
        },
        |(), elapsed_ms| {
            // 移除不可逆：键与整段耗时一并落 INFO，事后可核对删了什么
            tracing::info!(key = %key, elapsed_ms = elapsed_ms, "skill 移除完成");
        },
    )
}

/// remove 预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct RemovePlan {
    /// 要删的工作副本目录；不在场则不删、不提交
    workdir: Option<PathBuf>,
    /// 要删的归档副本目录；不在场则不删、不提交
    archive: Option<PathBuf>,
    /// 将执行的步骤清单
    actions: Vec<PlanStep>,
}

/// remove 预检：确认至少一侧副本在场、无人引用；动作清单在这里一次成形
fn prepare_remove(paths: &HubPaths, key: &str) -> Result<RemovePlan, SkillError> {
    let occupancy = skill_occupancy(paths, key);
    // 至少一侧在场：两侧皆无报 skill_not_found
    occupancy.require_removable()?;
    // 彻底删除前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    let workdir = occupancy.workdir().map(Path::to_path_buf);
    let archive = occupancy.archive().map(Path::to_path_buf);
    let mut actions = Vec::new();
    if workdir.is_some() {
        actions.push(PlanStep::of(
            ActionKind::Remove,
            crate::naming::store_subpath(key),
        ));
        actions.push(PlanStep::commit(message(Verb::Remove, Face::Store, key)));
    }
    if archive.is_some() {
        actions.push(PlanStep::of(
            ActionKind::Remove,
            crate::naming::archive_subpath(key),
        ));
        actions.push(PlanStep::commit(message(Verb::Remove, Face::Archive, key)));
    }
    Ok(RemovePlan {
        workdir,
        archive,
        actions,
    })
}
