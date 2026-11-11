//! 两入口（使用、撤回）共用的执行骨架件：相同前导（键校验、目标解析、
//! 无目标拒绝、预演分叉、取锁）与执行尾声（登记簿落盘失败回滚、里程碑日志）。
//! 只在本模块内部消费，不进对外接口。

use std::path::Path;
use std::time::Instant;

use crate::git;
use crate::hub;
use crate::lock::WriteToken;
use crate::paths::HubPaths;
use crate::project::target::anchors_of;
use crate::project::{Anchor, ProjectError, Targets};

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
        let paths = HubPaths::from_root(hub_root);
        let repo = git::open_hub(hub_root)?;
        Session::Dry { repo, paths }
    } else {
        let (token, repo, paths) = git::open_hub_for_write(hub_root)?;
        Session::Write { token, repo, paths }
    };
    Ok(Lead { anchors, session })
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

/// 尾声日志的一个目标条目
pub(super) struct Milestone<'a> {
    /// 目标身份
    pub(super) anchor: &'a Anchor,
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
            Some(message) => entry.anchor.log_skip(key, entry.count, elapsed_ms, message),
            None => entry
                .anchor
                .log_milestone(key, entry.count, elapsed_ms, entry.done),
        }
    }
}

/// 写命令机器提交消息 `<动词>(<目标>): <对象>` 的拼装：目标取去重后的
/// agent 名单，对象取键与逐目标的项目名（全局目标写「全局」）。
/// 消息只允许相对标识——项目写目录名，不写绝对路径
pub(super) fn commit_message(
    verb: &str,
    key: &str,
    agents: &[String],
    anchors: &[&Anchor],
) -> String {
    let mut names: Vec<String> = Vec::new();
    for anchor in anchors {
        let name = match anchor {
            Anchor::Project(dir) => dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project".to_string()),
            Anchor::Global => "global".to_string(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    format!(
        "{}({}): {} -> {}",
        verb,
        agents.join(","),
        key,
        names.join(",")
    )
}
