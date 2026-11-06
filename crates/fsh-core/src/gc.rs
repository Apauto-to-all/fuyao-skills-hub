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
    // 全局写锁：gc 会删导出目录、改登记簿，与其他写入口互斥；
    // dry-run 无副作用，但统一持锁语义更简单（拿不到锁 = 有人正在写）
    let token = crate::lock::write_lock(&paths)?;
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

    // ③ 死记录落地（dry-run 不改登记簿）
    if !dry_run && live_projects.len() != registry.projects.len() {
        let mut next = registry.clone();
        next.projects = live_projects;
        hub::save_registry(&paths, &next, &token)?;
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
mod tests {
    use super::*;
    use crate::git as git_mod;
    use crate::link;

    /// 建一个带单版本技能的 hub：工作副本 + tag + 手工导出，无任何项目引用
    fn hub_with_skill() -> (tempfile::TempDir, HubPaths) {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        let repo = git2::Repository::open(paths.root()).unwrap();
        let wd = paths.skill_workdir("demo");
        fs::create_dir_all(&wd).unwrap();
        fs::write(wd.join("SKILL.md"), "# demo\n").unwrap();
        git_mod::commit_all(&repo, "add demo", "fuyao", "fuyao@local").unwrap();
        git_mod::tag_version(&repo, "demo", "1.0.0").unwrap();
        git_mod::materialize(
            &repo,
            "demo/v1.0.0",
            "store/demo",
            &paths.exported("demo", "1.0.0"),
        )
        .unwrap();
        (install, paths)
    }

    /// 在登记簿里登记一个项目，并声明它引用了某技能某版本
    fn registry_referencing(
        project: &Path,
        key: &str,
        version: Option<&str>,
    ) -> hub::ProjectRegistry {
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(project, 1);
        reg.projects[0].skills.push(hub::ProjectSkill {
            key: key.into(),
            version: version.map(str::to_string),
            agents: vec!["agents".into()],
            link: link::LinkKind::Junction,
        });
        reg
    }

    #[test]
    fn gc_cleans_unreferenced_but_keeps_referenced() {
        let (install, paths) = hub_with_skill();
        // 另打一个 2.0.0 版本：无引用 → 应被清
        let repo = git2::Repository::open(paths.root()).unwrap();
        fs::write(paths.skill_workdir("demo").join("SKILL.md"), "# demo v2\n").unwrap();
        git_mod::commit_all(&repo, "v2", "fuyao", "fuyao@local").unwrap();
        git_mod::tag_version(&repo, "demo", "2.0.0").unwrap();
        git_mod::materialize(
            &repo,
            "demo/v2.0.0",
            "store/demo",
            &paths.exported("demo", "2.0.0"),
        )
        .unwrap();

        // 项目 A 引用 1.0.0：登记簿写死版本即构成引用
        let proj = tempfile::tempdir().unwrap();
        let reg = registry_referencing(proj.path(), "demo", Some("1.0.0"));
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

        let report = gc(paths.root(), false).unwrap();
        // 1.0.0 有引用保留，2.0.0 无引用被清
        assert!(paths.exported("demo", "1.0.0").exists(), "有引用不得清");
        assert!(!paths.exported("demo", "2.0.0").exists(), "无引用应被清");
        let cleaned = report
            .items
            .iter()
            .find_map(|i| match i {
                GcAction::Cleaned { path, .. } => Some(path),
                _ => None,
            })
            .expect("应记录一条清理动作");
        // 界面按对象列展示路径，路径必须指向被清的导出目录
        assert_eq!(cleaned, &paths.exported("demo", "2.0.0").to_string_lossy());
        // tag 永不清
        assert!(
            git_mod::tag_list(&repo, "demo")
                .unwrap()
                .contains(&"2.0.0".to_string())
        );
        let _ = install;
    }

    #[test]
    fn gc_treats_latest_tracking_record_as_no_export_reference() {
        let (install, paths) = hub_with_skill();
        // 项目跟最新（不写版本）：链接直指工作副本，不构成对导出目录的引用
        let proj = tempfile::tempdir().unwrap();
        let reg = registry_referencing(proj.path(), "demo", None);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(
            !paths.exported("demo", "1.0.0").exists(),
            "跟最新不引用导出目录，应可清"
        );
        let cleaned = report
            .items
            .iter()
            .find_map(|i| match i {
                GcAction::Cleaned { path, .. } => Some(path),
                _ => None,
            })
            .expect("应记录一条清理动作");
        // 界面按对象列展示路径，路径必须指向被清的导出目录
        assert_eq!(cleaned, &paths.exported("demo", "1.0.0").to_string_lossy());
        let _ = install;
    }

    #[test]
    fn gc_skips_dirty_and_cleans_orphan() {
        let (install, paths) = hub_with_skill();
        // 把 1.0.0 导出内容改脏 → 跳过报警
        fs::write(
            paths.exported("demo", "1.0.0").join("SKILL.md"),
            "tampered\n",
        )
        .unwrap();
        // 塞一个无 tag 的孤儿目录
        let orphan = paths.export().join("ghost").join("9.9.9");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("x.txt"), "x").unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(paths.exported("demo", "1.0.0").exists(), "脏目录绝不销毁");
        assert!(!orphan.exists(), "孤儿目录应被清");
        let dirty = report
            .items
            .iter()
            .find_map(|i| match i {
                GcAction::DirtySkipped { path, .. } => Some(path),
                _ => None,
            })
            .expect("脏目录应有跳过记录");
        let orphan_path = report
            .items
            .iter()
            .find_map(|i| match i {
                GcAction::OrphanCleaned { path, .. } => Some(path),
                _ => None,
            })
            .expect("孤儿目录应有清理记录");
        // 界面按对象列展示路径，两条记录都要指向实际目录
        assert_eq!(dirty, &paths.exported("demo", "1.0.0").to_string_lossy());
        assert_eq!(orphan_path, &orphan.to_string_lossy());
        let _ = install;
    }

    #[test]
    fn gc_removes_dead_records_and_honors_grace() {
        let (install, paths) = hub_with_skill();
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(Path::new("Z:/definitely/not/here"), 100);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

        // 宽限 0（默认）：死记录直接移除
        let report = gc(paths.root(), false).unwrap();
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::DeadRecordRemoved { .. }))
        );
        assert!(hub::load_registry(&paths).unwrap().projects.is_empty());

        // 宽限 30 天且记录很新：保留
        let mut reg2 = hub::ProjectRegistry::default();
        reg2.upsert(Path::new("Z:/definitely/not/here"), hub::unix_now());
        hub::save_registry(&paths, &reg2, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        let mut cfg = hub::load_config(&paths).unwrap().unwrap();
        cfg.gc.grace_days = 30;
        hub::save_config(&paths, &cfg).unwrap();
        let report2 = gc(paths.root(), false).unwrap();
        assert!(
            report2
                .items
                .iter()
                .any(|i| matches!(i, GcAction::GraceKept { .. }))
        );
        assert_eq!(hub::load_registry(&paths).unwrap().projects.len(), 1);
        let _ = install;
    }

    #[test]
    fn gc_dry_run_touches_nothing() {
        let (install, paths) = hub_with_skill();
        let exported = paths.exported("demo", "1.0.0");
        let reg_before = fs::read_to_string(paths.projects_toml()).ok();
        let report = gc(paths.root(), true).unwrap();
        assert!(exported.exists(), "dry-run 不得动盘");
        assert!(report.freed_bytes > 0, "dry-run 仍要报告可释放量");
        assert_eq!(
            fs::read_to_string(paths.projects_toml()).ok(),
            reg_before,
            "dry-run 不得改登记簿"
        );
        let _ = install;
    }

    #[test]
    fn gc_grace_keeps_fresh_unreferenced_dir() {
        let (install, paths) = hub_with_skill();
        // 宽限期拉满：刚导出的目录（mtime=现在）必须被留下
        let mut cfg = hub::load_config(&paths).unwrap().unwrap();
        cfg.gc.grace_days = 365;
        hub::save_config(&paths, &cfg).unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::GraceKept { .. })),
            "宽限期内不得清目录"
        );
        assert!(
            !report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::Cleaned { .. })),
            "宽限期内不应有 Cleaned"
        );
        assert!(paths.exported("demo", "1.0.0").exists(), "目录应保留");
        let _ = install;
    }

    #[test]
    fn gc_walks_nested_keys_and_cleans_orphans() {
        let (install, paths) = hub_with_skill();
        let repo = git2::Repository::open(paths.root()).unwrap();
        // 嵌套键技能：工作副本 + tag + 手工导出，三层目录都在 export 下
        let wd = paths.skill_workdir("anthropics/skills/pdf");
        fs::create_dir_all(&wd).unwrap();
        fs::write(wd.join("SKILL.md"), "# pdf\n").unwrap();
        git_mod::commit_all(&repo, "add pdf", "fuyao", "fuyao@local").unwrap();
        git_mod::tag_version(&repo, "anthropics/skills/pdf", "1.0.0").unwrap();
        git_mod::materialize(
            &repo,
            "anthropics/skills/pdf/v1.0.0",
            "store/anthropics/skills/pdf",
            &paths.exported("anthropics/skills/pdf", "1.0.0"),
        )
        .unwrap();
        // 嵌套键下的孤儿版本目录（无 tag）：递归收集应找到并清掉
        let orphan = paths
            .export()
            .join("anthropics")
            .join("skills")
            .join("pdf")
            .join("9.9.9");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("x.txt"), "x").unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(!orphan.exists(), "嵌套键下的孤儿导出目录应被清掉");
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::OrphanCleaned { .. })),
            "孤儿清理应有动作记录: {:?}",
            report.items
        );
        // 有 tag 且内容一致的嵌套导出目录：按无引用正常清，不再残留
        assert!(
            !paths.exported("anthropics/skills/pdf", "1.0.0").exists(),
            "嵌套键的导出目录应被递归收集后按规则清掉"
        );
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::Cleaned { .. })),
            "嵌套键导出目录应记为 Cleaned: {:?}",
            report.items
        );
        let _ = install;
    }

    #[test]
    fn gc_rejects_hub_without_full_assembly() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        fs::remove_file(paths.hub_toml()).unwrap();
        // 装配不完整：给出稳定错误码，而不是 git2 的原始错误
        match gc(paths.root(), true) {
            Err(GcError::HubNotInitialized(p)) => {
                assert_eq!(p, paths.root().display().to_string())
            }
            Err(other) => panic!("错误类型不对: {other}"),
            Ok(_) => panic!("半截 hub 不得被 gc 接受"),
        }
    }
}
