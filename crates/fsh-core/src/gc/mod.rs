//! 全量回收：以项目登记簿为唯一引用来源，清无引用导出目录、孤儿目录与
//! 死项目记录。dry-run 只列清单不动盘。清理永远无损——tag 在 git 里，
//! 导出目录可按字节重建；脏目录（内容与 tag 不符）是可疑证据，绝不销毁。

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::fingerprint;
use crate::git;
use crate::hub;
use crate::paths::HubPaths;

/// 全量回收错误
#[derive(Debug, Error)]
pub enum GcError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// 登记簿读写失败
    #[error("登记簿错误: {0}")]
    Registry(#[from] hub::HubError),
    /// 进程级文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] crate::lock::LockError),
    /// hub 未完整初始化
    #[error("hub 未初始化或装配不完整: {0}")]
    HubNotInitialized(String),
}

impl ErrorCode for GcError {
    fn error_code(&self) -> &'static str {
        match self {
            GcError::Io(_) => "io",
            GcError::Git(g) => g.error_code(),
            GcError::Registry(h) => h.error_code(),
            GcError::Lock(l) => l.error_code(),
            GcError::HubNotInitialized(_) => "hub_not_initialized",
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

/// 引用对集合：登记簿里写死版本的技能（不写版本的跟随最新，链接直指工作
/// 副本，不产生导出目录，因此不构成对导出目录的引用）
fn referenced_versions(registry: &hub::ProjectRegistry) -> HashSet<(String, String)> {
    registry
        .projects
        .iter()
        .flat_map(|p| p.skills.iter())
        .filter_map(|s| s.version.as_ref().map(|v| (s.key.clone(), v.clone())))
        .collect()
}

/// 全量回收：登记簿死记录按宽限移除，再扫 export\\ 分类处置。
/// git tag 与提交历史永不清。dry_run = true 时零磁盘副作用、零登记簿改动。
pub fn gc(hub_root: &Path, dry_run: bool) -> Result<GcReport, GcError> {
    let started = Instant::now();
    let paths = HubPaths::from_root(hub_root);
    // 装配不完整时给出与其余入口一致的稳定码，而不是 git2 的原始错误
    if !paths.is_initialized() {
        return Err(GcError::HubNotInitialized(hub_root.display().to_string()));
    }
    let repo = git2::Repository::open(hub_root).map_err(git::GitError::from)?;
    // 预演只读：不取写锁，别人正在写时也能给出估算，不会把「锁被占」误报成 0；
    // 实跑要删导出目录、改登记簿，才与其他写入口互斥
    let token = if dry_run {
        None
    } else {
        Some(crate::lock::write_lock(&paths)?)
    };
    let registry = hub::load_registry(&paths)?;
    let now = hub::unix_now();
    let grace_secs = load_grace_days(hub_root)? as u64 * 86_400;

    // ① 死记录：项目路径已不存在，过宽限移除、宽限内保留
    let mut items = Vec::new();
    let mut live_projects = Vec::new();
    for record in &registry.projects {
        let path_exists = Path::new(&record.path).is_dir();
        if path_exists {
            live_projects.push(record.clone());
            continue;
        }
        // 宽限只对带时间戳的对象生效
        let in_grace = grace_secs > 0 && record.last_used + grace_secs > now;
        if in_grace {
            items.push(GcAction::GraceKept {
                path: record.path.clone(),
            });
            live_projects.push(record.clone());
        } else {
            items.push(GcAction::DeadRecordRemoved {
                path: record.path.clone(),
            });
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

/// 项目使用成功后的顺手清：持锁期间按最新登记簿清扫无引用导出目录。
/// 入参凭证确认调用方已持全局写锁；本函数不再消费该凭证。
/// 失败不阻断项目使用——回收是派生数据整理，可随时再跑全量 gc。
pub(crate) fn sweep_after_use(
    repo: &git2::Repository,
    paths: &HubPaths,
    _token: &crate::lock::WriteToken,
) -> Result<GcReport, GcError> {
    let registry = hub::load_registry(paths)?;
    let referenced = referenced_versions(&registry);
    let grace_secs = load_grace_days(paths.root())? as u64 * 86_400;
    let (items, freed_bytes) = sweep(repo, paths, &referenced, grace_secs, false)?;
    Ok(GcReport { items, freed_bytes })
}

/// 参数化清扫——分类规则单一来源：登记簿说没人用才看 tag，tag 不在 =
/// 孤儿，内容不符 = 脏跳过报警，一致才删。无引用目录同样过宽限期才可
/// 删——后悔药旋钮对导出目录生效；时间读不到（含时钟偏移导致的未来
/// 时间戳）一律按「刚动过」处理：宁可不清，不可误删。
/// dry_run = true 时零磁盘副作用。返回动作清单与释放字节。
pub(crate) fn sweep(
    repo: &git2::Repository,
    paths: &HubPaths,
    referenced: &HashSet<(String, String)>,
    grace_secs: u64,
    dry_run: bool,
) -> Result<(Vec<GcAction>, u64), GcError> {
    // 候选：export\<键>\<三段数字版本>；键可多级，递归收集，
    // 版本目录内容是导出副本、不再下钻
    let mut candidates: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    collect_version_dirs(&paths.export(), "", &mut candidates);

    let mut items = Vec::new();
    let mut freed = 0u64;
    for (skill_key, vname, dir) in candidates {
        let bytes = dir_size(&dir);
        let tag = crate::naming::version_tag(&skill_key, &vname);
        if referenced.contains(&(skill_key.clone(), vname.clone())) {
            continue;
        }
        let mtime_age = fs::metadata(&dir)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if grace_secs > 0 && mtime_age < grace_secs {
            items.push(GcAction::GraceKept {
                path: dir.to_string_lossy().to_string(),
            });
            continue;
        }
        // 登记簿说没人用：tag 不在 = 孤儿；tag 在但内容不符 = 脏，跳过报警
        let tag_hash = fingerprint::tag_digest(repo, &tag, &skill_key);
        match tag_hash {
            None => {
                freed += bytes;
                items.push(GcAction::OrphanCleaned {
                    path: dir.to_string_lossy().to_string(),
                    bytes,
                });
                if !dry_run {
                    // 批量删除的外部交互：逐目录记落点，失败在同一处记 WARN 后原样上抛
                    fs::remove_dir_all(&dir).inspect_err(|e| {
                        tracing::warn!(path = %dir.display(), cause = %e, "回收清理导出目录失败");
                    })?;
                    tracing::debug!(path = %dir.display(), bytes, "回收清理导出目录");
                }
            }
            Some(h) => {
                let disk = fingerprint::dir_digest_best_effort(&dir);
                if disk.as_deref() == Some(h.as_str()) {
                    freed += bytes;
                    items.push(GcAction::Cleaned {
                        path: dir.to_string_lossy().to_string(),
                        bytes,
                    });
                    if !dry_run {
                        // 批量删除的外部交互：逐目录记落点，失败在同一处记 WARN 后原样上抛
                        fs::remove_dir_all(&dir).inspect_err(|e| {
                            tracing::warn!(path = %dir.display(), cause = %e, "回收清理导出目录失败");
                        })?;
                        tracing::debug!(path = %dir.display(), bytes, "回收清理导出目录");
                    }
                } else {
                    // 决策：内容与 tag 不符的目录可能是手改证据，跳过不删，留 WARN 供人回看
                    tracing::warn!(path = %dir.display(), bytes, "回收脏目录跳过");
                    items.push(GcAction::DirtySkipped {
                        path: dir.to_string_lossy().to_string(),
                        bytes,
                    });
                }
            }
        }
    }
    Ok((items, freed))
}

/// 递归收集导出候选：目录名是版本段（三段数字）即 (键前缀, 版本, 路径)
/// 候选，否则视为键前缀层继续下钻；版本目录内容是导出副本、不再下钻
fn collect_version_dirs(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, String, std::path::PathBuf)>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if crate::version::Version::parse(&name).is_some() {
            out.push((prefix.to_string(), name, p));
        } else {
            collect_version_dirs(&p, &rel, out);
        }
    }
}

/// 读 gc.grace_days；hub 未初始化或读不到按 0（即时清）
fn load_grace_days(hub_root: &Path) -> Result<u32, GcError> {
    let paths = HubPaths::from_root(hub_root);
    Ok(hub::load_config(&paths)?
        .map(|c| c.gc.grace_days)
        .unwrap_or(0))
}

/// 目录占用字节数（文件大小总和）
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(md) = p.symlink_metadata() {
                total += md.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests;
