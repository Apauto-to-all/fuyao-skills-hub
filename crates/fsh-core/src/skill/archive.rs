//! 归档：把技能的工作副本整目录移入 hub 平级的归档目录（archive\<键>\，
//! 与 store 同构、tracked）；技能仍被引用时拒绝。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SkillError, ensure_unreferenced, skill_status};
use crate::git;
use crate::paths::HubPaths;

/// archive 的结果载荷：实跑直出 `{key, archived}`，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，实跑与预演两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 归档的 skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 归档完成标志（仅实跑出现，恒为 true）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub archived: bool,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}

/// 归档：先查登记簿，被项目引用即拒绝；通过后把工作副本整目录移入
/// 归档目录（目标键已占用报 conflict），并自动提交。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn archive(hub_root: &Path, key: &str, dry_run: bool) -> Result<ArchiveOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    if dry_run {
        let paths = HubPaths::from_root(hub_root);
        let repo = git::open_hub(hub_root)?;
        let plan = prepare_archive(&repo, &paths, key)?;
        return Ok(ArchiveOutcome {
            dry_run: true,
            actions: plan.actions,
            ..ArchiveOutcome::default()
        });
    }
    let started = std::time::Instant::now();
    let (_token, repo, paths) = git::open_hub_for_write(hub_root)?;
    let plan = prepare_archive(&repo, &paths, key)?;
    if plan.had_workdir {
        let dst = paths.skill_archived(key);
        // 嵌套键的父层目录一并建出（archive/builtin/... 这类中间层）
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(paths.skill_workdir(key), &dst)?;
        git::auto_commit(&repo, &format!("archive(store): {key}"));
    }
    // 归档跨移动目录与机器提交两段外部操作：一条 INFO 交代键与整段耗时
    tracing::info!(
        key = %key,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "skill 归档完成"
    );
    Ok(ArchiveOutcome {
        key: key.into(),
        archived: true,
        ..ArchiveOutcome::default()
    })
}

/// 归档预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct ArchivePlan {
    /// 工作副本是否在场
    had_workdir: bool,
    /// 将执行的动作清单
    actions: Vec<String>,
}

/// 归档预检：确认目标活跃、无人引用、归档侧无同名键；动作清单在这里一次成形
fn prepare_archive(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<ArchivePlan, SkillError> {
    let status = skill_status(repo, paths, key)?;
    status.require_active()?;
    // 归档前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    // 归档侧同名键已占用：统一 conflict，不做静默改名
    if paths.skill_archived(key).exists() {
        return Err(SkillError::Conflict { key: key.into() });
    }
    let actions = vec![
        format!(
            "move {} -> {}",
            crate::naming::store_subpath(key),
            crate::naming::archive_subpath(key)
        ),
        format!("commit: archive(store): {key}"),
    ];
    Ok(ArchivePlan {
        had_workdir: true,
        actions,
    })
}
