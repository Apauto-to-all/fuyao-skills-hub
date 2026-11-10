//! 归档：删工作副本与导出目录，版本 tag 与 git 历史全留。
//! 与 remove 相对——归档可 restore，remove 连 tag 一起删。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{SkillError, SkillState, ensure_unreferenced, identity, skill_status};
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

/// 归档：先查登记簿，被项目引用即拒绝；通过后删工作副本与导出目录，
/// tag 与历史全留，可 restore。
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
        fs::remove_dir_all(paths.skill_workdir(key))?;
        let id = identity(&paths)?;
        git::commit_all(&repo, &format!("archive {key}"), &id.name, &id.email)?;
    }
    remove_export_dirs(&paths, key)?;
    // 归档跨删工作副本、提交、清导出目录三类外部操作：一条 INFO 交代键与整段耗时
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
    /// 工作副本是否在场（决定要不要删掉并提交）
    had_workdir: bool,
    /// 将执行的动作清单
    actions: Vec<String>,
}

/// 归档预检：确认目标存在、且无人引用；动作清单在这里一次成形
fn prepare_archive(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<ArchivePlan, SkillError> {
    let status = skill_status(repo, paths, key)?;
    status.require_present()?;
    let had_workdir = status.state() == SkillState::Active;
    // 归档前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    let mut actions = Vec::new();
    if had_workdir {
        actions.push(format!("remove {}", crate::naming::store_subpath(key)));
        actions.push(format!("commit: archive {key}"));
    }
    // 导出目录的根内相对形态进文案：布局知识只在 paths；正斜杠让动作文案跨平台同形
    let export_subpath = paths
        .export()
        .join(key)
        .strip_prefix(paths.root())
        .expect("导出目录由 hub 根拼出，必在其内")
        .to_string_lossy()
        .replace('\\', "/");
    actions.push(format!("remove {export_subpath}/*"));
    Ok(ArchivePlan {
        had_workdir,
        actions,
    })
}

/// 删该键的全部导出目录；目录不在视为已清（幂等）
fn remove_export_dirs(paths: &HubPaths, key: &str) -> Result<(), SkillError> {
    let export = paths.export().join(key);
    match fs::remove_dir_all(&export) {
        Ok(()) => Ok(()),
        // 导出目录本就不在属幂等命中：DEBUG 留痕，区分「没人用过」与「被别处提前清掉」
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::debug!(path = %export.display(), "导出目录不在，无需清理");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
