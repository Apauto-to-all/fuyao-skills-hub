//! 全量回收：以项目登记簿为唯一引用来源，清无引用导出目录、孤儿目录与
//! 死项目记录。dry-run 只列清单不动盘。清理永远无损——tag 在 git 里，
//! 导出目录可按字节重建；脏目录（内容与 tag 不符）是可疑证据，绝不销毁。

mod sweep;
mod walk;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::error::{CommonError, common_error_from};
use crate::git;
use crate::hub;
use crate::paths::HubPaths;

pub(crate) use sweep::{referenced_versions, sweep, sweep_after_use};

/// 全量回收错误：底层失败（文件系统、git、文件锁、登记簿）全走公共底层错误
#[derive(Debug, Error)]
pub enum GcError {
    /// 底层失败
    #[error(transparent)]
    Common(#[from] CommonError),
}

common_error_from!(
    GcError: std::io::Error,
    git::GitError,
    hub::HubError,
    crate::lock::LockError
);

impl ErrorCode for GcError {
    fn error_code(&self) -> &'static str {
        match self {
            GcError::Common(c) => c.error_code(),
        }
    }
}

/// 单条回收动作（dry-run 时表示「将会发生」，执行时表示「已发生」）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GcAction {
    /// 无引用且内容与 tag 一致的导出目录：已清（或 dry-run 将清）；path 为目录绝对路径
    Cleaned { path: String, bytes: u64 },
    /// 无对应 tag 的孤儿目录：已清（或 dry-run 将清）；path 为目录绝对路径
    OrphanCleaned { path: String, bytes: u64 },
    /// 内容与 tag 不符：跳过并报警（可能是手改证据，绝不销毁）；path 为目录绝对路径
    DirtySkipped { path: String, bytes: u64 },
    /// 登记簿里路径已不存在的死项目记录：已移除
    DeadRecordRemoved { path: String },
    /// 死项目记录在宽限期内：保留
    GraceKept { path: String },
}

/// 回收报告：全部动作 + 释放（或可释放）字节数
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GcReport {
    /// 全部动作清单
    pub items: Vec<GcAction>,
    /// 清理释放（dry-run 为潜在释放）的总字节数
    pub freed_bytes: u64,
}

/// 全量回收：登记簿死记录按宽限移除，再扫 export\\ 分类处置。
/// git tag 与提交历史永不清。dry_run = true 时零磁盘副作用、零登记簿改动。
pub fn gc(hub_root: &Path, dry_run: bool) -> Result<GcReport, GcError> {
    let started = Instant::now();
    let paths = HubPaths::from_root(hub_root);
    // 装配不完整的判定与稳定码由 git::open_hub 单点持有；预演只读：
    // 不取写锁，别人正在写时也能给出估算，不会把「锁被占」误报成 0；
    // 实跑要删导出目录、改登记簿，才与其他写入口同走写姿势（判定 → 取锁 → 开仓）
    let (token, repo) = if dry_run {
        (None, git::open_hub(hub_root)?)
    } else {
        let (token, repo, _) = git::open_hub_for_write(hub_root)?;
        (Some(token), repo)
    };
    let registry = hub::load_registry(&paths)?;
    let now = hub::unix_now();
    let grace_secs = load_grace_days(hub_root)? as u64 * 86_400;

    // ① 死记录：项目路径已不存在，过宽限移除、宽限内保留
    let mut items = Vec::new();
    let mut live_projects = std::collections::BTreeMap::new();
    for (path, record) in &registry.projects {
        if Path::new(path).is_dir() {
            live_projects.insert(path.clone(), record.clone());
            continue;
        }
        // 宽限只对带时间戳的对象生效
        let in_grace = grace_secs > 0 && record.last_used + grace_secs > now;
        if in_grace {
            items.push(GcAction::GraceKept { path: path.clone() });
            live_projects.insert(path.clone(), record.clone());
        } else {
            items.push(GcAction::DeadRecordRemoved { path: path.clone() });
        }
    }

    // ② 扫 export\\ 分类处置（全 hub 范围，配置宽限）
    let referenced = referenced_versions(&registry);
    let (sweep_items, freed) = sweep(&repo, &paths, &referenced, grace_secs, dry_run)?;
    items.extend(sweep_items);

    // ③ 死记录落地（预演无锁也无副作用）
    if let Some(token) = token.as_ref()
        && live_projects.len() != registry.projects.len()
    {
        let mut next = registry.clone();
        next.projects = live_projects;
        hub::save_registry(&paths, &next, token)?;
    }

    let report = GcReport {
        items,
        freed_bytes: freed,
    };
    // 状态迁移与整次耗时：一条 INFO 重建本次回收结果；dry_run 区分预演与实删，避免把预演读成真删
    tracing::info!(
        path = %paths.root().display(),
        count = report.items.len(),
        freed_bytes = report.freed_bytes,
        dry_run,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "回收完成"
    );
    Ok(report)
}

/// 读 gc.grace_days；hub 未初始化或读不到按 0（即时清）
fn load_grace_days(hub_root: &Path) -> Result<u32, GcError> {
    let paths = HubPaths::from_root(hub_root);
    Ok(hub::load_config(&paths)?
        .map(|c| c.gc.grace_days)
        .unwrap_or(0))
}
