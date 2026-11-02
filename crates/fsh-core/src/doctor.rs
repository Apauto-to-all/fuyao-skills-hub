//! 自检：八项健康检查，只报不动手——诊断命令永远不可能弄坏数据。
//! 输出结构化报告（检查项 + ok/warn/fail + 说明），GUI 与 CLI 直接渲染。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::gc;
use crate::hub;
use crate::link;
use crate::paths::HubPaths;

/// 自检错误（单条检查失败不算：那是 fail 状态的检查项，不是错误）
#[derive(Debug, Error)]
pub enum DoctorError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
}

impl ErrorCode for DoctorError {
    fn error_code(&self) -> &'static str {
        match self {
            DoctorError::Io(_) => "io",
        }
    }
}

/// 单条检查结论
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoctorCheck {
    /// 检查项名（稳定标识，供 --json 消费方匹配）
    pub name: String,
    /// 结论
    pub status: CheckStatus,
    /// 人读说明
    pub detail: String,
}

/// 检查结论三态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// 通过
    Ok,
    /// 可疑但可用（有兜底或暂不致命）
    Warn,
    /// 不满足运行前提
    Fail,
}

/// 自检报告
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DoctorReport {
    /// 全部检查项
    pub checks: Vec<DoctorCheck>,
}

/// 可回收空间提醒阈值（字节）：超过即提示该跑 gc
pub const RECLAIM_WARN_BYTES: u64 = 100 * 1024 * 1024;

/// 云同步路径关键词：hub 落在这些目录里会被云占位符破坏链接引擎。
/// ponytail: 关键词匹配是检测下限；reparse 属性级检测在真机出现漏报时再升级
const CLOUD_DIR_KEYWORDS: [&str; 5] = [
    "onedrive",
    "dropbox",
    "google drive",
    "googledrive",
    "icloud",
];

/// 运行八项自检：安装目录可写 / 非云同步路径 / 非 Program Files /
/// 链接能力实测三级 / 有 lock 引用但物化缺失 / 项目不可达 /
/// 可回收超阈值 / 超 N 天未备份。只报不动手。
pub fn doctor(install_dir: &Path) -> Result<DoctorReport, DoctorError> {
    let paths = HubPaths::new(install_dir);
    let hub_root = paths.root();
    let mut checks = Vec::with_capacity(8);

    // ① 安装目录可写：在 hub 根写一个探针文件再删
    let writable = (|| -> Result<(), String> {
        fs::create_dir_all(hub_root).map_err(|e| e.to_string())?;
        let probe = hub_root.join(".fsh-doctor-probe");
        fs::write(&probe, b"").map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&probe);
        Ok(())
    })();
    checks.push(match writable {
        Ok(()) => DoctorCheck {
            name: "install_writable".into(),
            status: CheckStatus::Ok,
            detail: "hub 目录可写".into(),
        },
        Err(e) => DoctorCheck {
            name: "install_writable".into(),
            status: CheckStatus::Fail,
            detail: format!("hub 目录不可写: {e}"),
        },
    });

    // ② 云同步路径：关键词匹配（小写比较）
    let path_str = hub_root.to_string_lossy().to_lowercase();
    let cloud_hit = CLOUD_DIR_KEYWORDS.iter().find(|k| path_str.contains(*k));
    checks.push(DoctorCheck {
        name: "cloud_path".into(),
        status: if cloud_hit.is_some() {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail: match cloud_hit {
            Some(k) => format!("hub 位于云同步路径（{k}），链接会被云占位符破坏"),
            None => "hub 不在云同步路径".into(),
        },
    });

    // ③ Program Files：权限受限的系统目录
    let pf_hit = path_str.contains("\\program files");
    checks.push(DoctorCheck {
        name: "program_files".into(),
        status: if pf_hit {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail: if pf_hit {
            "hub 位于 Program Files 下，写入将受系统权限限制".into()
        } else {
            "hub 不在 Program Files 下".into()
        },
    });

    // ④ 链接能力：真实走一遍三级降级
    checks.push(match test_link_capability() {
        Ok(kind) => DoctorCheck {
            name: "link_capability".into(),
            status: CheckStatus::Ok,
            detail: format!("本机链接档位: {kind:?}"),
        },
        Err(e) => DoctorCheck {
            name: "link_capability".into(),
            status: CheckStatus::Fail,
            detail: format!("三级降级全部失败: {e}"),
        },
    });

    // ⑤ 物化缺失：可达项目的 lock 引用的版本目录应在盘上
    let set = crate::refs::reference_set(&paths).unwrap_or_default();
    let mut missing = Vec::new();
    for r in &set.references {
        if !paths.materialized(&r.key, &r.version).is_dir() {
            missing.push(format!("{}@{}", r.key, r.version));
        }
    }
    checks.push(DoctorCheck {
        name: "materialized_missing".into(),
        status: if missing.is_empty() {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: if missing.is_empty() {
            "登记在案的版本目录全部在位".into()
        } else {
            format!(
                "有 lock 引用但物化缺失（下次 apply 自动重物化）: {}",
                missing.join(", ")
            )
        },
    });

    // ⑥ 项目不可达：目录在但 lock 读不到（缺文件 / 损坏）——不贡献
    //    引用，需人工核对（引用账本在此断了，gc 与 purge 守卫都看不见它）
    let reason_of = |reason: crate::refs::UnreachableReason| match reason {
        crate::refs::UnreachableReason::LockMissing => "缺 lock",
        crate::refs::UnreachableReason::LockCorrupted => "lock 损坏",
    };
    checks.push(DoctorCheck {
        name: "unreachable_projects".into(),
        status: if set.unreachable.is_empty() {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: if set.unreachable.is_empty() {
            "登记在案的项目全部可达".into()
        } else {
            format!(
                "项目 lock 读不到（不贡献引用）: {}",
                set.unreachable
                    .iter()
                    .map(|u| format!("{}（{}）", u.path, reason_of(u.reason)))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
    });

    // ⑦ 可回收空间：gc dry-run 的潜在释放量超阈值即提醒
    let reclaimable = if hub_root.join(".git").is_dir() {
        gc::gc(hub_root, true).map(|r| r.freed_bytes).unwrap_or(0)
    } else {
        0
    };
    checks.push(DoctorCheck {
        name: "reclaimable".into(),
        status: if reclaimable > RECLAIM_WARN_BYTES {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        detail: format!("可回收 {reclaimable} 字节（阈值 {}）", RECLAIM_WARN_BYTES),
    });

    // ⑧ 备份节奏：从未备份或超过 remind_days 未备份
    let registry = hub::load_registry(&paths).unwrap_or_default();
    let remind_days = hub::load_config(&paths)
        .ok()
        .flatten()
        .map(|c| c.backup.remind_days)
        .unwrap_or_else(hub::default_remind_days) as u64;
    let now = hub::unix_now();
    let (st, detail) = if registry.last_backup == 0 {
        (CheckStatus::Warn, "从未备份".to_string())
    } else {
        let age_days = now.saturating_sub(registry.last_backup) / 86_400;
        if age_days > remind_days {
            (
                CheckStatus::Warn,
                format!("{age_days} 天未备份（提醒阈值 {remind_days} 天）"),
            )
        } else {
            (CheckStatus::Ok, format!("上次备份 {age_days} 天前"))
        }
    };
    checks.push(DoctorCheck {
        name: "backup_stale".into(),
        status: st,
        detail,
    });

    Ok(DoctorReport { checks })
}

/// 链接能力实测：临时目录里真建一次链接，返回实际档位
fn test_link_capability() -> Result<link::LinkKind, String> {
    // 同进程内并发调用（并行测试）会共用临时目录，追加原子序号保证互不干扰
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("fsh-doctor-link-{}-{seq}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("linked");
    let _ = fs::remove_dir_all(&base);
    let result = (|| -> Result<link::LinkKind, String> {
        fs::create_dir_all(&src).map_err(|e| e.to_string())?;
        fs::write(src.join("probe"), b"").map_err(|e| e.to_string())?;
        link::create_dir_link(&src, &dst, "probe").map_err(|e| e.to_string())
    })();
    let _ = link::remove_link(&dst);
    let _ = fs::remove_dir_all(&base);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_hub_gets_eight_checks_with_expected_statuses() {
        let install = tempfile::tempdir().unwrap();
        hub::hub_init(install.path()).unwrap();
        let report = doctor(install.path()).unwrap();
        let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "install_writable",
                "cloud_path",
                "program_files",
                "link_capability",
                "materialized_missing",
                "unreachable_projects",
                "reclaimable",
                "backup_stale",
            ]
        );
        // 临时目录环境下的预期：可写 ok、云盘 ok、PF ok、链接 ok、
        // 无登记 ok、无不可达 ok、无垃圾 ok、从未备份 warn
        let status_of = |n: &str| report.checks.iter().find(|c| c.name == n).unwrap().status;
        assert_eq!(status_of("install_writable"), CheckStatus::Ok);
        assert_eq!(status_of("cloud_path"), CheckStatus::Ok);
        assert_eq!(status_of("program_files"), CheckStatus::Ok);
        assert_eq!(status_of("link_capability"), CheckStatus::Ok);
        assert_eq!(status_of("materialized_missing"), CheckStatus::Ok);
        assert_eq!(status_of("unreachable_projects"), CheckStatus::Ok);
        assert_eq!(status_of("reclaimable"), CheckStatus::Ok);
        assert_eq!(
            status_of("backup_stale"),
            CheckStatus::Warn,
            "从未备份应提醒"
        );
    }

    #[test]
    fn backup_freshens_stale_warning() {
        let install = tempfile::tempdir().unwrap();
        hub::hub_init(install.path()).unwrap();
        let dest = tempfile::tempdir().unwrap();
        crate::backup::backup(HubPaths::new(install.path()).root(), dest.path()).unwrap();
        let report = doctor(install.path()).unwrap();
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "backup_stale")
            .unwrap();
        assert_eq!(check.status, CheckStatus::Ok, "刚备份完不应提醒");
    }

    #[test]
    fn missing_materialization_is_reported() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        // 登记 + lock 引用一个盘上不存在的版本
        let proj = tempfile::tempdir().unwrap();
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(proj.path(), 1);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        let lock = crate::deploy::LockFile {
            entries: vec![crate::deploy::LockEntry {
                key: "demo".into(),
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
        let report = doctor(install.path()).unwrap();
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "materialized_missing")
            .unwrap();
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(check.detail.contains("demo@1.0.0"));
    }

    #[test]
    fn unreachable_project_is_warned_with_reason() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        // 登记一个目录在但没 lock 的项目：doctor 应告警不可达并说明原因
        let proj = tempfile::tempdir().unwrap();
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(proj.path(), 1);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

        let report = doctor(install.path()).unwrap();
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "unreachable_projects")
            .unwrap();
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(
            check.detail.contains("缺 lock"),
            "应说明原因: {}",
            check.detail
        );

        // lock 损坏同属不可达
        std::fs::write(HubPaths::project_lock(proj.path()), "broken = [").unwrap();
        let report = doctor(install.path()).unwrap();
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "unreachable_projects")
            .unwrap();
        assert_eq!(check.status, CheckStatus::Warn);
        assert!(
            check.detail.contains("lock 损坏"),
            "应说明原因: {}",
            check.detail
        );
    }

    #[test]
    fn doctor_flags_cloud_path() {
        // 路径注入构造云同步命中态：安装目录落在含 OneDrive 字样的路径下
        let base = tempfile::tempdir().unwrap();
        let install = base.path().join("OneDriveSync").join("hub");
        fs::create_dir_all(&install).unwrap();
        hub::hub_init(&install).unwrap();
        let report = doctor(&install).unwrap();
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "cloud_path")
            .unwrap();
        assert_eq!(
            check.status,
            CheckStatus::Warn,
            "云同步路径应告警: {}",
            check.detail
        );
    }
}
