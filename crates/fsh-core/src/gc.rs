//! 全量回收：以项目登记簿为引用账本，清无引用物化目录、孤儿目录与
//! 死项目记录。dry-run 只列清单不动盘。清理永远无损——tag 在 git 里，
//! 物化目录可按字节重建；脏目录（内容与 tag 不符）是可疑证据，绝不销毁。
//! 清扫规则（分类 + 宽限 + 不可达语义）住在本模块，gc 与 deploy 顺手清同源。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::deploy::DeployError;
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
    /// 部署侧读取失败（登记簿 / lock）
    #[error("部署读取错误: {0}")]
    Deploy(#[from] DeployError),
    /// 登记簿读写失败
    #[error("登记簿错误: {0}")]
    Registry(#[from] hub::HubError),
    /// 进程级文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] crate::lock::LockError),
}

impl ErrorCode for GcError {
    fn error_code(&self) -> &'static str {
        match self {
            GcError::Io(_) => "io",
            GcError::Git(g) => g.error_code(),
            GcError::Deploy(d) => d.error_code(),
            GcError::Registry(h) => h.error_code(),
            GcError::Lock(l) => l.error_code(),
        }
    }
}

/// 单条回收动作（dry-run 时表示「将会发生」，执行时表示「已发生」）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GcAction {
    /// 无引用且内容与 tag 一致的物化目录：已清（或 dry-run 将清）
    Cleaned { bytes: u64 },
    /// 无对应 tag 的孤儿目录：已清（或 dry-run 将清）
    OrphanCleaned { bytes: u64 },
    /// 内容与 tag 不符：跳过并报警（可能是手改证据，绝不销毁）
    DirtySkipped { bytes: u64 },
    /// 登记簿里路径已不存在的死项目记录：已移除
    DeadRecordRemoved { path: String },
    /// 死项目记录在宽限期内：保留
    GraceKept { path: String },
    /// 登记项不可达（项目残缺缺 lock / lock 损坏）：按无引用处理，
    /// 记录保留并告警
    UnreachableProject { path: String },
}

/// 回收报告：全部动作 + 释放（或可释放）字节数
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GcReport {
    /// 全部动作清单
    pub items: Vec<GcAction>,
    /// 清理释放（dry-run 为潜在释放）的总字节数
    pub freed_bytes: u64,
}

/// 全量回收：引用集一次走查登记簿（refs 模块），死记录按宽限移除，
/// 不可达告警且记录保留；再扫 store\ 分类处置。
/// git tag 与提交历史永不清。dry_run = true 时零磁盘副作用、零登记簿改动。
pub fn gc(hub_root: &Path, dry_run: bool) -> Result<GcReport, GcError> {
    let paths = HubPaths::from_root(hub_root);
    let repo = git2::Repository::open(hub_root).map_err(git::GitError::from)?;
    // 全局写锁：gc 会删物化目录、改登记簿，与其他写入口互斥；
    // dry-run 无副作用，但统一持锁语义更简单（拿不到锁 = 有人正在写）
    let token = crate::lock::write_lock(&paths)?;
    let registry = hub::load_registry(&paths)?;
    let now = hub::unix_now();
    let grace_secs = load_grace_days(hub_root)? as u64 * 86_400;

    // ① 引用集：不可达一律不贡献引用；死记录过宽限移除、宽限内保留；
    //    不可达项目（缺 lock / lock 损坏）记录一律保留，只告警
    let set = crate::refs::reference_set(&paths)?;
    let mut items = Vec::new();
    let mut removed_paths = Vec::new();
    for d in &set.dead_records {
        // 宽限只对带时间戳的对象生效
        let in_grace = grace_secs > 0 && d.last_applied + grace_secs > now;
        if in_grace {
            items.push(GcAction::GraceKept {
                path: d.path.clone(),
            });
        } else {
            items.push(GcAction::DeadRecordRemoved {
                path: d.path.clone(),
            });
            removed_paths.push(d.path.clone());
        }
    }
    for u in &set.unreachable {
        items.push(GcAction::UnreachableProject {
            path: u.path.clone(),
        });
    }
    let live_records: Vec<hub::ProjectRecord> = registry
        .projects
        .iter()
        .filter(|r| !removed_paths.contains(&r.path))
        .cloned()
        .collect();

    // ② 扫 store\ 分类处置（全 hub 范围，配置宽限）
    let (sweep_items, freed) = sweep(
        &repo,
        &paths,
        &token,
        &set,
        SweepScope::AllHub,
        grace_secs,
        dry_run,
    )?;
    items.extend(sweep_items);

    // ③ 死记录落地（dry-run 不改登记簿）
    if !dry_run && live_records.len() != registry.projects.len() {
        let mut next = registry.clone();
        next.projects = live_records;
        hub::save_registry(&paths, &next, &token)?;
    }

    Ok(GcReport {
        items,
        freed_bytes: freed,
    })
}

/// 清扫范围：全 hub（gc）或单个技能的全部版本目录（顺手清）
pub(crate) enum SweepScope<'a> {
    /// 全 hub：store\ 下所有技能的所有版本目录
    AllHub,
    /// 指定技能的全部版本目录
    Skill(&'a str),
}

/// 参数化清扫——分类规则单一来源：引用集说没人用才看 tag，tag 不在 =
/// 孤儿，内容不符 = 脏跳过报警，一致才删；引用中的目录跳过（单技能
/// 范围下记 INFO 日志，供顺手清观测跨项目保护）。无引用目录同样过
/// 宽限期才可删——后悔药旋钮对物化目录生效；时间读不到（含时钟偏移
/// 导致的未来时间戳）一律按「刚动过」处理：宁可不清，不可误删。
/// dry_run = true 时零磁盘副作用。返回动作清单与释放字节。
pub(crate) fn sweep(
    repo: &git2::Repository,
    paths: &HubPaths,
    _token: &crate::lock::WriteToken,
    set: &crate::refs::ReferenceSet,
    scope: SweepScope<'_>,
    grace_secs: u64,
    dry_run: bool,
) -> Result<(Vec<GcAction>, u64), GcError> {
    // 范围内候选：每个技能目录下的三段数字版本目录
    let skill_dirs: Vec<std::path::PathBuf> = match scope {
        SweepScope::AllHub => fs::read_dir(paths.store())
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default(),
        SweepScope::Skill(name) => vec![paths.store().join(name)],
    };
    let mut candidates: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    for skill_dir in skill_dirs {
        if !skill_dir.is_dir() {
            continue;
        }
        let skill_name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(versions) = fs::read_dir(&skill_dir) else {
            continue;
        };
        for ver in versions.flatten() {
            let vname = ver.file_name().to_string_lossy().to_string();
            if !ver.path().is_dir() || !is_version_dir_name(&vname) {
                continue;
            }
            candidates.push((skill_name.clone(), vname, ver.path()));
        }
    }

    let mut items = Vec::new();
    let mut freed = 0u64;
    for (skill_name, vname, dir) in candidates {
        let bytes = dir_size(&dir);
        let tag = crate::naming::version_tag(&skill_name, &vname);
        if set.is_referenced(&skill_name, &vname) {
            if matches!(scope, SweepScope::Skill(_)) {
                tracing::info!(
                    skill = %skill_name,
                    version = %vname,
                    "清扫跳过引用中的物化目录"
                );
            }
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
        // 引用集说没人用：tag 不在 = 孤儿；tag 在但内容不符 = 脏，跳过报警
        let tag_hash = hash_tag(repo, &tag, &skill_name);
        match tag_hash {
            None => {
                freed += bytes;
                items.push(GcAction::OrphanCleaned { bytes });
                if !dry_run {
                    fs::remove_dir_all(&dir)?;
                }
            }
            Some(h) => {
                let disk = hash_dir_best_effort(&dir);
                if disk.as_deref() == Some(h.as_str()) {
                    freed += bytes;
                    items.push(GcAction::Cleaned { bytes });
                    if !dry_run {
                        fs::remove_dir_all(&dir)?;
                    }
                } else {
                    items.push(GcAction::DirtySkipped { bytes });
                }
            }
        }
    }
    Ok((items, freed))
}

/// 顺手清：apply 末尾对本次触达技能的即时清扫。引用集取全登记簿口径
/// （含刚登记的当前项目），宽限 0；失败由调用方吞掉——apply 主体不
/// 回滚，残留交给 gc 兜底。
pub(crate) fn sweep_touched(
    repo: &git2::Repository,
    paths: &HubPaths,
    token: &crate::lock::WriteToken,
    touched: &[String],
) -> Result<(), GcError> {
    let set = crate::refs::reference_set(paths)?;
    for name in touched {
        sweep(repo, paths, token, &set, SweepScope::Skill(name), 0, false)?;
    }
    Ok(())
}

/// 物化版本目录名判定：三段数字（semver 目录），每段按 u64 解析；
/// 非三段数字目录是工作副本等编辑面内容，清扫一律不碰
fn is_version_dir_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| p.parse::<u64>().is_ok())
}

/// 读 gc.grace_days；hub 未初始化或读不到按 0（即时清）
fn load_grace_days(hub_root: &Path) -> Result<u32, GcError> {
    let paths = HubPaths::from_root(hub_root);
    Ok(hub::load_config(&paths)?
        .map(|c| c.gc.grace_days)
        .unwrap_or(0))
}

/// tag 子树内容哈希；tag 不存在返回 None
fn hash_tag(repo: &git2::Repository, tag: &str, skill: &str) -> Option<String> {
    let subpath = crate::naming::store_subpath(skill);
    crate::deploy::hash_tag_contents(repo, tag, &subpath).ok()
}

/// 目录内容哈希；读不了的目录按「不匹配」处理（交给脏目录报警）
fn hash_dir_best_effort(dir: &Path) -> Option<String> {
    crate::deploy::hash_dir_contents(dir).ok()
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

    /// 建一个带单版本技能的 hub：工作副本 + tag + 手动物化，无任何项目引用
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
            &paths.materialized("demo", "1.0.0"),
        )
        .unwrap();
        (install, paths)
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
            &paths.materialized("demo", "2.0.0"),
        )
        .unwrap();

        // 项目 A 引用 1.0.0：登记簿 + 项目侧 lock（结构体序列化，格式与 apply 写出一致）
        let proj = tempfile::tempdir().unwrap();
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(proj.path(), 1);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        let lock = crate::deploy::LockFile {
            entries: vec![crate::deploy::LockEntry {
                name: "demo".into(),
                version: "1.0.0".into(),
                commit: "x".into(),
                tree_hash: "y".into(),
                kind: link::LinkKind::Junction,
                dst: ".fsh/skills/demo".into(),
            }],
        };
        fs::write(
            HubPaths::project_lock(proj.path()),
            toml::to_string(&lock).unwrap(),
        )
        .unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(
            !report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::UnreachableProject { .. })),
            "lock 可读的项目不得记为不可达"
        );
        // 1.0.0 有引用保留，2.0.0 无引用被清
        assert!(paths.materialized("demo", "1.0.0").exists(), "有引用不得清");
        assert!(
            !paths.materialized("demo", "2.0.0").exists(),
            "无引用应被清"
        );
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::Cleaned { .. }))
        );
        // tag 永不清
        assert!(
            git_mod::tag_list(&repo, "demo")
                .unwrap()
                .contains(&"2.0.0".to_string())
        );
        let _ = install;
    }

    #[test]
    fn gc_skips_dirty_and_cleans_orphan() {
        let (install, paths) = hub_with_skill();
        // 把 1.0.0 物化内容改脏 → 跳过报警
        fs::write(
            paths.materialized("demo", "1.0.0").join("SKILL.md"),
            "tampered\n",
        )
        .unwrap();
        // 塞一个无 tag 的孤儿目录
        let orphan = paths.store().join("ghost").join("9.9.9");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("x.txt"), "x").unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(
            paths.materialized("demo", "1.0.0").exists(),
            "脏目录绝不销毁"
        );
        assert!(!orphan.exists(), "孤儿目录应被清");
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::DirtySkipped { .. }))
        );
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::OrphanCleaned { .. }))
        );
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
        let mat = paths.materialized("demo", "1.0.0");
        let reg_before = fs::read_to_string(paths.projects_toml()).ok();
        let report = gc(paths.root(), true).unwrap();
        assert!(mat.exists(), "dry-run 不得动盘");
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
        // 宽限期拉满：刚物化的目录（mtime=现在）必须被留下
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
        assert!(paths.materialized("demo", "1.0.0").exists(), "目录应保留");
        let _ = install;
    }

    #[test]
    fn gc_keeps_record_when_lock_corrupted() {
        let (install, paths) = hub_with_skill();
        // lock 文件存在但内容损坏：告警不可达，登记簿记录必须保留
        let proj = tempfile::tempdir().unwrap();
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(proj.path(), 1);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        fs::write(HubPaths::project_lock(proj.path()), "not = [valid").unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::UnreachableProject { .. })),
            "lock 损坏应告警不可达"
        );
        assert_eq!(
            hub::load_registry(&paths).unwrap().projects.len(),
            1,
            "lock 损坏不得销登记记录"
        );
        let _ = install;
    }

    #[test]
    fn gc_warns_unreachable_project_as_unreferenced() {
        let (install, paths) = hub_with_skill();
        // 登记一个路径存在但没有 lock 的「项目」：读不到 lock → 告警 + 按无引用
        let empty_proj = tempfile::tempdir().unwrap();
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(empty_proj.path(), 1);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

        let report = gc(paths.root(), false).unwrap();
        assert!(
            report
                .items
                .iter()
                .any(|i| matches!(i, GcAction::UnreachableProject { .. }))
        );
        assert!(
            !paths.materialized("demo", "1.0.0").exists(),
            "不可达项目不构成引用"
        );
        let _ = install;
    }
}
