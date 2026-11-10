//! 九项检查各自的实现：只读环境与盘面，每项返回一条结论。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::builtin::BuiltinSkill;
use crate::gc;
use crate::git;
use crate::hub;
use crate::link;
use crate::naming;
use crate::paths::HubPaths;

use super::{CheckStatus, DoctorCheck};

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

/// ① hub 根可写：在 hub 根写一个探针文件再删
pub(super) fn hub_writable(paths: &HubPaths) -> DoctorCheck {
    let hub_root = paths.root();
    let writable = (|| -> Result<(), String> {
        fs::create_dir_all(hub_root).map_err(|e| e.to_string())?;
        let probe = hub_root.join(".fsh-doctor-probe");
        fs::write(&probe, b"").map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&probe);
        Ok(())
    })();
    match writable {
        Ok(()) => DoctorCheck {
            name: "hub_writable".into(),
            status: CheckStatus::Ok,
            detail: "hub 根可写".into(),
        },
        // 单项 FAIL 是诊断结论而非致命故障：记 WARN 留痕，不升级为 ERROR
        Err(e) => {
            tracing::warn!(
                path = %hub_root.display(),
                cause = %e,
                "hub 根可写性检查失败"
            );
            DoctorCheck {
                name: "hub_writable".into(),
                status: CheckStatus::Fail,
                detail: format!("hub 根不可写: {e}"),
            }
        }
    }
}

/// ② 云同步路径：关键词匹配（小写比较）
pub(super) fn cloud_path(hub_root: &Path) -> DoctorCheck {
    let path_str = hub_root.to_string_lossy().to_lowercase();
    let cloud_hit = CLOUD_DIR_KEYWORDS.iter().find(|k| path_str.contains(*k));
    DoctorCheck {
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
    }
}

/// ③ 链接能力：真实走一遍三级降级
pub(super) fn link_capability() -> DoctorCheck {
    match test_link_capability() {
        Ok(kind) => DoctorCheck {
            name: "link_capability".into(),
            status: CheckStatus::Ok,
            detail: format!("本机链接档位: {kind}"),
        },
        // 链接档位全断是单项 FAIL：记 WARN 指明本机降级链失效
        Err(e) => {
            tracing::warn!(cause = %e, "链接能力实测失败");
            DoctorCheck {
                name: "link_capability".into(),
                status: CheckStatus::Fail,
                detail: format!("三级降级全部失败: {e}"),
            }
        }
    }
}

/// ④ hub 装配与可读性：缺件 / hub.toml 解析 / 仓库可打开 / 忽略规则一致。
/// 判据只管存在，这里才管「存在但坏」——诊断命令不修，只把坏法说清楚
pub(super) fn hub_integrity(paths: &HubPaths) -> DoctorCheck {
    let mut broken: Vec<String> = Vec::new();
    let missing = paths.missing_parts();
    if !missing.is_empty() {
        broken.push(format!("装配不完整（缺 {}）", missing.join(" / ")));
    }
    if paths.hub_toml().is_file()
        && let Err(e) = hub::load_config(paths)
    {
        broken.push(format!("hub.toml 解析失败（{e}）"));
    }
    if paths.root().join(".git").exists() && git::Repository::open(paths.root()).is_err() {
        broken.push("git 仓库打不开".into());
    }
    // 忽略规则被改坏只降级为告警：仓库仍可用，只是下次提交可能带进不该进的东西
    let drifted = paths.gitignore().is_file() && !hub::gitignore_is_canonical(paths);
    let status = if !broken.is_empty() {
        CheckStatus::Fail
    } else if drifted {
        CheckStatus::Warn
    } else {
        CheckStatus::Ok
    };
    let detail = if broken.is_empty() && !drifted {
        "hub 装配完整、仓库可打开".into()
    } else {
        let mut parts = broken;
        if drifted {
            parts.push(".gitignore 与白名单不符，重跑 init 重写".into());
        }
        parts.join("；")
    };
    DoctorCheck {
        name: "hub_integrity".into(),
        status,
        detail,
    }
}

/// 登记簿的一次读取结果：读不出时带原因、用空登记簿继续
pub(super) struct RegistryRead {
    /// 读到的登记簿（读不出时是空登记簿）
    pub(super) registry: hub::ProjectRegistry,
    /// 读不出的原因
    pub(super) error: Option<String>,
}

/// 读一次登记簿：读不出的原因在这里转成文本带出，不在上层重记
pub(super) fn read_registry(paths: &HubPaths) -> RegistryRead {
    match hub::load_registry(paths) {
        Ok(registry) => RegistryRead {
            registry,
            error: None,
        },
        Err(error) => RegistryRead {
            registry: hub::ProjectRegistry::default(),
            error: Some(error.to_string()),
        },
    }
}

/// ⑤ 内置技能铺到全局：一次都没进全局用法记录，AI 看不到它，等于没内置。
/// 只给可照抄的命令骨架，agent 由用户自己填——不替用户挑、不自动 use
pub(super) fn builtin_global(skills: &[BuiltinSkill], read: &RegistryRead) -> DoctorCheck {
    const NAME: &str = "builtin_global";
    // 登记簿读不出就判不出铺没铺：报原因，不盖章成 OK
    if let Some(error) = &read.error {
        return DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Warn,
            detail: format!("用法登记簿读不出，无法判定内置技能是否铺到全局（{error}）"),
        };
    }
    // 铺到全局 = 全局用法记录里点到这个键；给了哪些 agent 不论
    let registry = &read.registry;
    let missing: Vec<String> = skills
        .iter()
        .map(|skill| naming::builtin_key(&skill.name))
        .filter(|key| {
            !registry
                .global()
                .skills
                .iter()
                .any(|record| &record.key == key)
        })
        .collect();
    if missing.is_empty() {
        return DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Ok,
            detail: "内置技能已铺到全局".into(),
        };
    }
    let skeleton = missing
        .iter()
        .map(|key| format!("fsh use {key} --agent <你的 agent> --global"))
        .collect::<Vec<_>>()
        .join("；");
    DoctorCheck {
        name: NAME.into(),
        status: CheckStatus::Warn,
        detail: format!(
            "内置技能未铺到全局（AI 看不到它，等于没内置），照抄并填上你的 agent: {skeleton}"
        ),
    }
}

/// ⑥ 内置技能内容对齐：`store\builtin\` 与程序内嵌逐字节一致才算 OK。
/// 程序升级后常见不一致；目录整个缺失、内嵌里已无的孤儿目录同样算不一致
pub(super) fn builtin_content(paths: &HubPaths, skills: &[BuiltinSkill]) -> DoctorCheck {
    const NAME: &str = "builtin_content";
    let embedded: BTreeSet<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
    let mut drifted: Vec<String> = skills
        .iter()
        .filter(|skill| !hub::workdir_matches(paths, &naming::builtin_key(&skill.name), skill))
        .map(|skill| skill.name.clone())
        .collect();
    let orphans = match hub::orphan_names(paths, &embedded) {
        Ok(names) => names,
        // 读不了 store 就判不出对齐：报原因并指向 init，不猜 OK
        Err(error) => {
            return DoctorCheck {
                name: NAME.into(),
                status: CheckStatus::Warn,
                detail: format!("内置技能内容核对失败（{error}）：运行 fsh init 对齐"),
            };
        }
    };
    drifted.extend(orphans);
    if drifted.is_empty() {
        return DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Ok,
            detail: "内置技能内容与程序一致".into(),
        };
    }
    DoctorCheck {
        name: NAME.into(),
        status: CheckStatus::Warn,
        detail: format!(
            "内置技能内容与程序不一致（{}）：运行 fsh init 对齐；copy 落点需 unuse 后重铺",
            drifted.join(", ")
        ),
    }
}

/// ⑦⑧ 登记簿两项：导出目录缺失 + 项目路径不可达
pub(super) fn registry(paths: &HubPaths, read: &RegistryRead) -> [DoctorCheck; 2] {
    let registry = &read.registry;

    // ⑦ 导出目录缺失：登记簿里写死版本的技能，其导出目录应在盘上
    let mut missing = Vec::new();
    for skill in registry.projects.values().flat_map(|p| p.skills.iter()) {
        if let Some(version) = &skill.version
            && !paths.exported(&skill.key, version).is_dir()
        {
            missing.push(format!("{}@{}", skill.key, version));
        }
    }
    let exported = DoctorCheck {
        name: "exported_missing".into(),
        status: if missing.is_empty() {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: if missing.is_empty() {
            "登记在案的导出目录全部在位".into()
        } else {
            format!(
                "登记簿有引用但导出目录缺失（重新 use 即重建）: {}",
                missing.join(", ")
            )
        },
    };

    // ⑧ 登记簿不可达：项目路径已不存在（这条记录失效，gc 会按死记录处理），
    //    或登记簿本身读不出（整份登记簿失效，需人工修文件）
    let mut unreachable: Vec<String> = registry
        .projects
        .keys()
        .filter(|p| !Path::new(p.as_str()).is_dir())
        .cloned()
        .collect();
    if let Some(e) = &read.error {
        unreachable.push(format!("登记簿读不出（{e}）"));
    }
    let dead = DoctorCheck {
        name: "dead_projects".into(),
        status: if unreachable.is_empty() {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        },
        detail: if unreachable.is_empty() {
            "登记在案的项目路径全部存在".into()
        } else {
            format!("登记簿不可达: {}", unreachable.join(", "))
        },
    };

    [exported, dead]
}

/// ⑨ 可回收空间：gc dry-run 的潜在释放量超阈值即提醒。
/// 装配不完整时不跑 gc：它会拒绝半截 hub，可回收量交给 hub_integrity 报
pub(super) fn reclaimable(paths: &HubPaths) -> DoctorCheck {
    const NAME: &str = "reclaimable";
    if !paths.is_initialized() {
        return DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Ok,
            detail: "hub 未初始化，无可回收统计".into(),
        };
    }
    match gc::gc(paths.root(), true) {
        // 算不出来就报原因：印 0 会被读成「没有可回收的东西」
        Err(error) => DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Warn,
            detail: format!("可回收空间统计失败: {error}"),
        },
        Ok(report) => DoctorCheck {
            name: NAME.into(),
            status: if report.freed_bytes > RECLAIM_WARN_BYTES {
                CheckStatus::Warn
            } else {
                CheckStatus::Ok
            },
            detail: format!(
                "可回收 {} 字节（阈值 {RECLAIM_WARN_BYTES}）",
                report.freed_bytes
            ),
        },
    }
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
        link::create_dir_link(&src, &dst).map_err(|e| e.to_string())
    })();
    // 撤销按实测档位撤：链接只删自身，拷贝整目录删
    if let Ok(kind) = result {
        let _ = link::remove_link(&dst, kind);
    }
    let _ = fs::remove_dir_all(&base);
    result
}
