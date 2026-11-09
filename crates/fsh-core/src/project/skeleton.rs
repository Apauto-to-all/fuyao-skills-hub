//! 三入口（使用、撤回、版本切换）共用的执行骨架件：相同前导（键校验、目标解析、
//! 无目标拒绝、预演分叉、取锁）、导出目录就位（查、核、物化）与执行尾声
//! （登记簿落盘失败回滚、顺手回收、里程碑日志）。只在本模块内部消费，不进对外接口。

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::git;
use crate::hub;
use crate::lock::WriteToken;
use crate::paths::HubPaths;
use crate::project::dest::ensure_export_matches;
use crate::project::target::anchors_of;
use crate::project::{Anchor, CleanupReport, ProjectError, Targets, lock_and_open, open_repo};

/// 一次操作的前导结论：展开后的目标清单与分支句柄
pub(super) struct Lead {
    /// 展开后的目标清单，全局目标在最前
    pub(super) anchors: Vec<Anchor>,
    /// 预演分支只读不持锁；实跑分支持全局写锁
    pub(super) session: Session,
}

/// 前导分叉出的执行姿势
pub(super) enum Session {
    /// 预演：只读仓库句柄，不取写锁——别人正在写时照样能读
    Dry {
        repo: git::Repository,
        paths: HubPaths,
    },
    /// 实跑：全局写锁与仓库句柄，锁由调用方握到操作结束
    Write {
        token: WriteToken,
        repo: git::Repository,
        paths: HubPaths,
    },
}

/// 三入口相同的前导：键校验、目标解析、无目标拒绝、预演分叉、取锁。
/// 键先过形状判定（写错的键当场点名，不落到「没有登记」那种误导性报错；
/// CLI 之外的入口同样过这一道，规则不靠前端复写）；目标集合为空是参数错误，
/// 先于 hub 装配检查报出
pub(super) fn lead(
    hub_root: &Path,
    key: &str,
    targets: &Targets,
    dry_run: bool,
) -> Result<Lead, ProjectError> {
    crate::key::validate(key)?;
    let anchors = anchors_of(targets);
    if anchors.is_empty() {
        return Err(ProjectError::NoTarget);
    }
    let session = if dry_run {
        let (repo, paths) = open_repo(hub_root)?;
        Session::Dry { repo, paths }
    } else {
        let (token, repo, paths) = lock_and_open(hub_root)?;
        Session::Write { token, repo, paths }
    };
    Ok(Lead { anchors, session })
}

/// 目标版本的源就位结论
pub(super) struct SourceStage {
    /// 技能源目录（固定版本是导出目录，跟随最新是工作副本）
    pub(super) source: PathBuf,
    /// 固定版本的导出目录（跟随最新时为空）
    pub(super) export_dir: Option<PathBuf>,
    /// 固定版本（跟随最新时为空）
    pub(super) version: Option<String>,
}

/// 「固定版本链导出目录、跟随最新链工作副本」的唯一住所：目标版本要链哪个目录、
/// 已存在的导出目录先核对（与 tag 一致才复用，不一致保留现场报错）都从这里出
pub(super) fn stage_source(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
    version: Option<&str>,
) -> Result<SourceStage, ProjectError> {
    let Some(v) = version else {
        return Ok(SourceStage {
            source: paths.skill_workdir(key),
            export_dir: None,
            version: None,
        });
    };
    let dir = paths.exported(key, v);
    if dir.is_dir() {
        ensure_export_matches(repo, key, v, &dir)?;
    }
    Ok(SourceStage {
        source: dir.clone(),
        export_dir: Some(dir),
        version: Some(v.to_string()),
    })
}

/// 登记记录推导出的源目录：固定版本是它登记的导出目录，跟随最新是工作副本，
/// 与目标版本的取法同一条规则
pub(super) fn recorded_source(paths: &HubPaths, key: &str, version: Option<&str>) -> PathBuf {
    match version {
        Some(v) => paths.exported(key, v),
        None => paths.skill_workdir(key),
    }
}

/// 导出目录物化：目录还不在盘上时按版本 tag 重建。固定版本的操作在建链接 / 换靶前
/// 先走这里，源先就位，后续步骤不会因为源缺失半途而废
pub(super) fn materialize_export(
    repo: &git::Repository,
    key: &str,
    version: &str,
    dir: &Path,
) -> Result<(), ProjectError> {
    if dir.is_dir() {
        return Ok(());
    }
    let tag = crate::naming::version_tag(key, version);
    git::materialize(repo, &tag, &crate::naming::store_subpath(key), dir)?;
    Ok(())
}

/// 预演动作清单的导出行：导出目录还没就位时，每个目标的清单最前面各加一行导出动作
pub(super) fn prepend_export_actions<'a>(
    stage: &SourceStage,
    actions_of: impl Iterator<Item = &'a mut Vec<String>>,
) {
    let Some(dir) = stage.export_dir.as_ref().filter(|d| !d.is_dir()) else {
        return;
    };
    let line = format!(
        "export {} -> {}",
        stage.version.as_deref().unwrap_or(""),
        dir.display()
    );
    for actions in actions_of {
        actions.insert(0, line.clone());
    }
}

/// 登记簿落盘的外壳：读、改、写一次收口。读失败、改写函数报错或写盘失败时，
/// 先执行调用方给的盘面还原再原样上抛——同一次操作不留半套状态
pub(super) fn commit_registry(
    paths: &HubPaths,
    token: &WriteToken,
    restore: impl FnOnce(),
    mutate: impl FnOnce(&mut hub::ProjectRegistry) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    let mut registry = match hub::load_registry(paths) {
        Ok(registry) => registry,
        Err(e) => {
            restore();
            return Err(e.into());
        }
    };
    if let Err(e) = mutate(&mut registry) {
        restore();
        return Err(e);
    }
    if let Err(e) = hub::save_registry(paths, &registry, token) {
        restore();
        return Err(e.into());
    }
    Ok(())
}

/// 登记簿落定后的顺手回收：清无引用导出目录。失败不阻断操作，只留痕可查
pub(super) fn sweep_exports(
    repo: &git::Repository,
    paths: &HubPaths,
    token: &WriteToken,
    fail_message: &str,
) -> Option<CleanupReport> {
    match crate::gc::sweep_after_use(repo, paths, token) {
        Ok(report) => Some(CleanupReport {
            removed: report.items.len(),
            freed_bytes: report.freed_bytes,
        }),
        Err(e) => {
            tracing::warn!(cause = %e, "{}", fail_message);
            None
        }
    }
}

/// 尾声日志的一个目标条目
pub(super) struct Milestone<'a> {
    /// 目标身份
    pub(super) anchor: &'a Anchor,
    /// 记入日志的版本（跟随最新时为空）
    pub(super) version: Option<&'a str>,
    /// 该目标的落点数
    pub(super) count: usize,
    /// 动盘目标的完成消息（INFO 里程碑）
    pub(super) done: &'static str,
    /// 零盘动作目标的跳过消息（WARN）；None 表示这次操作没有零盘目标
    pub(super) skipped: Option<&'static str>,
}

/// 执行尾声的里程碑日志：逐目标各记一条，动盘的记完成、零盘的记跳过，
/// 读日志即可重建「哪个目标发生了什么」；耗时由同一次操作的全部条目共用
pub(super) fn log_milestones<'a, I>(key: &str, started: Instant, entries: I)
where
    I: IntoIterator<Item = Milestone<'a>>,
{
    let elapsed_ms = started.elapsed().as_millis() as u64;
    for entry in entries {
        match entry.skipped {
            // 零盘动作的目标没有可读的变更，日志里要说清为什么什么都没变
            Some(message) => {
                entry
                    .anchor
                    .log_skip(key, entry.version, entry.count, elapsed_ms, message)
            }
            None => {
                entry
                    .anchor
                    .log_milestone(key, entry.version, entry.count, elapsed_ms, entry.done)
            }
        }
    }
}
