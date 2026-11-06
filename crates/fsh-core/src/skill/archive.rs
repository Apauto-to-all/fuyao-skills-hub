//! 归档：删工作副本与导出目录，版本 tag 与 git 历史全留。
//! 与 remove 相对——归档可 restore，remove 连 tag 一起删。

use std::fs;
use std::path::Path;

use super::{SkillError, identity, lock_and_open, referencing_projects, workdir_exists};
use crate::git;
use crate::paths::HubPaths;

/// 归档：先查登记簿，被项目引用即拒绝；通过后删工作副本与导出目录，
/// tag 与历史全留，可 restore。
pub fn archive(hub_root: &Path, key: &str) -> Result<(), SkillError> {
    let started = std::time::Instant::now();
    let (_token, repo, paths) = lock_and_open(hub_root)?;
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
    Ok(())
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
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<ArchivePlan, SkillError> {
    let versions = git::tag_list(repo, key)?;
    let had_workdir = workdir_exists(paths, key);
    if versions.is_empty() && !had_workdir {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    // 归档前必须无人引用：登记簿说有项目在用就拒绝，不给强制开关
    let projects = referencing_projects(paths, key)?;
    if !projects.is_empty() {
        return Err(SkillError::Referenced {
            key: key.into(),
            projects,
        });
    }
    let mut actions = Vec::new();
    if had_workdir {
        actions.push(format!("remove {}", crate::naming::store_subpath(key)));
        actions.push(format!("commit: archive {key}"));
    }
    actions.push(format!(
        "remove {}/*",
        crate::naming::exported_key_subpath(key)
    ));
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

/// archive 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn archive_dry(hub_root: &Path, key: &str) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    Ok(prepare_archive(&repo, &paths, key)?.actions)
}
