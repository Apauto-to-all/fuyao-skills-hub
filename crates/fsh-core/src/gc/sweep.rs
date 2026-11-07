//! 参数化清扫：无引用导出目录的分类处置规则。
//! 分类规则单一来源：登记簿说没人用才看 tag，tag 不在 = 孤儿，内容不符 =
//! 脏跳过报警，一致才删。无引用目录同样过宽限期才可删——后悔药旋钮对
//! 导出目录生效；时间读不到（含时钟偏移导致的未来时间戳）一律按「刚
//! 动过」处理：宁可不清，不可误删。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use super::{GcAction, GcError, GcReport};
use crate::fingerprint;
use crate::git;
use crate::hub;
use crate::lock::WriteToken;
use crate::paths::HubPaths;

/// 引用对集合：登记簿里写死版本的技能（不写版本的跟随最新，链接直指工作
/// 副本，不产生导出目录，因此不构成对导出目录的引用）
pub(super) fn referenced_versions(registry: &hub::ProjectRegistry) -> HashSet<(String, String)> {
    registry
        .projects
        .values()
        .flat_map(|p| p.skills.iter())
        .filter_map(|s| s.version.as_ref().map(|v| (s.key.clone(), v.clone())))
        .collect()
}

/// 项目使用成功后的顺手清：持锁期间按最新登记簿清扫无引用导出目录。
/// 入参凭证确认调用方已持全局写锁；本函数不再消费该凭证。
/// 失败不阻断项目使用——回收是派生数据整理，可随时再跑全量 gc。
pub(crate) fn sweep_after_use(
    repo: &git::Repository,
    paths: &HubPaths,
    _token: &WriteToken,
) -> Result<GcReport, GcError> {
    let registry = hub::load_registry(paths)?;
    let referenced = referenced_versions(&registry);
    let grace_secs = super::load_grace_days(paths.root())? as u64 * 86_400;
    let (items, freed_bytes) = sweep(repo, paths, &referenced, grace_secs, false)?;
    Ok(GcReport { items, freed_bytes })
}

/// 参数化清扫——dry_run = true 时零磁盘副作用。返回动作清单与释放字节。
pub(crate) fn sweep(
    repo: &git::Repository,
    paths: &HubPaths,
    referenced: &HashSet<(String, String)>,
    grace_secs: u64,
    dry_run: bool,
) -> Result<(Vec<GcAction>, u64), GcError> {
    // 候选：export\<键>\<三段数字版本>；键可多级，递归收集，
    // 版本目录内容是导出副本、不再下钻
    let mut candidates: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    super::walk::collect_version_dirs(&paths.export(), "", &mut candidates);

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

/// 目录占用字节数（文件大小总和）
pub(super) fn dir_size(dir: &Path) -> u64 {
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
