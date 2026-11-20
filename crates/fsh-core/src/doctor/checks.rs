//! 各项检查的实现：本地各项只看一次扫描带出的事实，版本检查另看网络；
//! 每项返回一条结论；只报不动。

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs;

use crate::git;
use crate::hub;
use crate::link;
use crate::naming;
use crate::skill::{Occupancy, occupancy};
use crate::update::{self, CURRENT_VERSION};

use super::CheckStatus;
use super::DoctorCheck;
use super::scan::{FaceScan, Scan};

/// 云同步路径关键词：hub 落在这些目录里会被云占位符破坏链接引擎。
/// ponytail: 关键词匹配是检测下限；reparse 属性级检测在真机出现漏报时再升级
const CLOUD_DIR_KEYWORDS: [&str; 5] = [
    "onedrive",
    "dropbox",
    "google drive",
    "googledrive",
    "icloud",
];

/// 三态构造：没有问题给 Ok，有问题给 Warn，说明按问题清单成形。
/// 「空→ok / 非空→warn」的推导只住在这里
fn settle(
    name: &str,
    problems: &[String],
    ok: &str,
    warn: impl FnOnce(String) -> String,
) -> DoctorCheck {
    if problems.is_empty() {
        return DoctorCheck {
            name: name.into(),
            status: CheckStatus::Ok,
            detail: ok.into(),
        };
    }
    DoctorCheck {
        name: name.into(),
        status: CheckStatus::Warn,
        detail: warn(problems.join(", ")),
    }
}

/// 核对不了与核对通过是两种结论：不能核对的按可疑告警并点名原因
fn unverifiable(name: &str, detail: String) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status: CheckStatus::Warn,
        detail,
    }
}

/// ① hub 根可写：在 hub 根写一个探针文件再删
pub(super) fn hub_writable(scan: &Scan) -> DoctorCheck {
    let hub_root = scan.paths.root();
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
pub(super) fn cloud_path(scan: &Scan) -> DoctorCheck {
    let path_str = scan.paths.root().to_string_lossy().to_lowercase();
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
pub(super) fn hub_integrity(scan: &Scan) -> DoctorCheck {
    let paths = &scan.paths;
    let mut broken: Vec<String> = Vec::new();
    let missing = paths.missing_parts();
    if !missing.is_empty() {
        broken.push(format!("装配不完整（缺 {}）", missing.join(" / ")));
    }
    // 配置损坏的结论随扫描带出：本项报成 FAIL 并点名，同一次读取不再记第二条日志
    if let Err(e) = &scan.config {
        broken.push(e.clone());
    }
    // 仓库可打开性只在装配齐全时查验：缺件已由上一项点名，这里不叠加
    // 「未初始化」的重复报；坏仓库点名给 Fail
    if paths.is_initialized() && git::open_hub(paths.root()).is_err() {
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

/// ⑤ 内置技能铺到全局：一次都没进全局用法记录，AI 看不到它，等于没内置。
/// 只给可照抄的命令骨架，agent 由用户自己填——不替用户挑、不自动 use
pub(super) fn builtin_global(scan: &Scan) -> DoctorCheck {
    const NAME: &str = "builtin_global";
    // 登记簿读不出就判不出铺没铺：报原因，不盖章成 OK
    if let Some(error) = &scan.registry_error {
        return unverifiable(
            NAME,
            format!("用法登记簿读不出，无法判定内置技能是否铺到全局（{error}）"),
        );
    }
    // 铺到全局 = 全局用法记录里点到这个键；给了哪些 agent 不论
    let registry = &scan.registry;
    let missing: Vec<String> = scan
        .builtin
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
        .map(|key| use_skeleton(key))
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

/// 一条内置技能的可照抄铺装命令：键拆成前缀与末段，两段各走自己的参数
fn use_skeleton(key: &str) -> String {
    match key.rsplit_once('/') {
        Some((prefix, name)) => {
            format!("fsh use --prefix {prefix} --skill {name} --agent <你的 agent> --global")
        }
        None => format!("fsh use --skill {key} --agent <你的 agent> --global"),
    }
}

/// ⑥ 内置技能内容对齐：`store\builtin\` 与程序内嵌逐字节一致才算 OK。
/// 程序升级后常见不一致；目录整个缺失、内嵌里已无的孤儿目录同样算不一致
pub(super) fn builtin_content(scan: &Scan) -> DoctorCheck {
    const NAME: &str = "builtin_content";
    let paths = &scan.paths;
    let embedded: BTreeSet<&str> = scan
        .builtin
        .iter()
        .map(|skill| skill.name.as_str())
        .collect();
    let mut drifted: Vec<String> = scan
        .builtin
        .iter()
        .filter(|skill| !hub::workdir_matches(paths, &naming::builtin_key(&skill.name), skill))
        .map(|skill| skill.name.clone())
        .collect();
    let orphans = match hub::orphan_names(paths, &embedded) {
        Ok(names) => names,
        // 读不了 store 就判不出对齐：报原因并指向 init，不猜 OK
        Err(error) => {
            return unverifiable(
                NAME,
                format!("内置技能内容核对失败（{error}）：运行 fsh init 对齐"),
            );
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

/// 登记簿不可达：项目路径已不存在，或登记簿本身读不出（需人工修文件）。
/// 本项只报告；确认不要的记录由 unuse 清
pub(super) fn registry(scan: &Scan) -> DoctorCheck {
    let mut unreachable: Vec<String> = scan
        .registry
        .project_dirs()
        .filter(|dir| !dir.is_dir())
        .map(|dir| dir.display().to_string())
        .collect();
    if let Some(e) = &scan.registry_error {
        unreachable.push(format!("登记簿读不出（{e}）"));
    }
    settle(
        "dead_projects",
        &unreachable,
        "登记在案的项目路径全部存在",
        |list| format!("登记簿不可达: {list}"),
    )
}

/// 孤儿落点：登记簿里的技能记录解析出的落点在盘上已不存在。
/// 解析不出来时不能当成「没有落点」——结论按「无法核对」告警，只报不动；
/// 确认不要的记录用 unuse 清
pub(super) fn orphan_destinations(scan: &Scan) -> DoctorCheck {
    const NAME: &str = "orphan_destinations";
    let dests = match &scan.destinations {
        Ok(dests) => dests,
        // 核对不了与核对通过是两种结论：这里按可疑告警，并点名原因
        Err(error) => return unverifiable(NAME, format!("无法核对孤儿落点（{error}）")),
    };
    let missing: Vec<String> = dests
        .iter()
        .filter(|dest| occupancy(&dest.path) == Occupancy::Nothing)
        .map(|dest| dest.path.display().to_string())
        .collect();
    settle(NAME, &missing, "登记在案的落点全部在位", |list| {
        format!("孤儿落点（盘上已不存在）: {list}")
    })
}

/// 孤儿目录：store 与 archive 面下不含 SKILL.md 的残留目录。
/// 只报不动；确认不要的目录手工删除
pub(super) fn orphan_dirs(scan: &Scan) -> DoctorCheck {
    let mut orphans = Vec::new();
    for face in &scan.faces {
        collect_orphan_dirs(face, &mut orphans);
    }
    settle(
        "orphan_dirs",
        &orphans,
        "store / archive 下没有孤儿目录",
        |list| format!("孤儿目录（不含 SKILL.md 的残留目录）: {list}"),
    )
}

/// 键末段与 frontmatter name 一致性：对两面里每个含 SKILL.md 的技能目录断言
/// 键末段 = frontmatter name。漂移只报不动；改身份走整目录改名 + 重新 add，
/// 撤销则把 name 改回
pub(super) fn name_drift(scan: &Scan) -> DoctorCheck {
    let mut drifted: Vec<String> = Vec::new();
    for face in &scan.faces {
        collect_name_drift(face, &mut drifted);
    }
    settle(
        "name_drift",
        &drifted,
        "键末段与 frontmatter name 全部一致",
        |list| {
            format!(
                "键末段与 frontmatter name 不一致: {list}。改身份走整目录改名 + 重新 add；撤销则把 name 改回"
            )
        },
    )
}

/// ⑪ 程序版本是否有更新：唯一一项看网络的检查，且强制刷新——用户敲 doctor
/// 问的就是「现在是什么情况」，读一天前的缓存等于撒谎。只报不动：不写状态文件
pub(super) fn update_available(scan: &Scan) -> DoctorCheck {
    const NAME: &str = "update_available";
    let cfg = match scan.update_config() {
        Ok(cfg) => cfg,
        // 配置读不出就判定不了档位：核对不了，不替用户猜一个发布源去联网
        Err(e) => return unverifiable(NAME, format!("无法核对版本（{e}）")),
    };
    // 明确关掉就尊重这个选择：诊断命令不反复唠叨同一件用户已经决定的事
    if !cfg.mode.queries_in_doctor() {
        return DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Ok,
            detail: "版本检查已关闭".into(),
        };
    }
    let latest = match update::probe(&cfg) {
        Ok(latest) => latest,
        // 查不到不等于通过：核对不了按可疑告警并点名发布源与原因
        Err(e) => {
            return unverifiable(
                NAME,
                format!("无法核对版本（发布源 {}：{e}）", cfg.registry),
            );
        }
    };
    match update::compare(&latest, CURRENT_VERSION) {
        Ok(Ordering::Greater) => DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Warn,
            detail: format!(
                "有新版本 {latest}（当前 {CURRENT_VERSION}），升级: {}",
                update::upgrade_command()
            ),
        },
        Ok(_) => DoctorCheck {
            name: NAME.into(),
            status: CheckStatus::Ok,
            detail: format!("已是最新（当前 {CURRENT_VERSION}）"),
        },
        // 版本串读不懂也是核对不了：不能据此盖章成通过
        Err(reason) => unverifiable(NAME, format!("无法核对版本（{reason}）")),
    }
}

/// 收集面内键末段与 frontmatter name 不一致或读不出的技能目录
fn collect_name_drift(face: &FaceScan, out: &mut Vec<String>) {
    for skill in face.skills.iter().filter(|skill| skill.has_skill_md) {
        let label = format!("{}/{}", face.name, skill.key);
        let dir_name = crate::key::last_segment(&skill.key);
        let parsed = fs::read_to_string(skill.dir.join("SKILL.md"))
            .map_err(|e| e.to_string())
            .and_then(|text| {
                crate::skill::frontmatter::parse(&text, &label)
                    .map(|fm| fm.name)
                    .map_err(|e| match &e {
                        crate::skill::SkillError::InvalidSource { reason, .. } => reason.clone(),
                        other => other.to_string(),
                    })
            });
        match parsed {
            Ok(fm_name) if fm_name == dir_name => {}
            Ok(fm_name) => out.push(format!("{label}（name={fm_name}）")),
            Err(reason) => out.push(format!("{label}（无法核对: {reason}）")),
        }
    }
}

/// 收集面内的孤儿目录：自身与子树都没有 SKILL.md 的目录，只点名最外层
fn collect_orphan_dirs(face: &FaceScan, out: &mut Vec<String>) {
    // 子树里没有技能根才是孤儿：前缀层带着技能就不点名
    let mut orphans: Vec<&str> = face
        .skills
        .iter()
        .filter(|entry| !entry.has_skill_md)
        .map(|entry| entry.key.as_str())
        .filter(|key| {
            !face
                .skills
                .iter()
                .any(|skill| skill.has_skill_md && skill.key.starts_with(&format!("{key}/")))
        })
        .collect();
    // 只点名最外层：键按字典序排好，父键必在子键之前，已点名的父吃掉自己的子
    orphans.sort_unstable();
    let mut kept: Vec<&str> = Vec::new();
    for key in orphans {
        if kept
            .iter()
            .any(|outer| key.starts_with(&format!("{outer}/")))
        {
            continue;
        }
        kept.push(key);
    }
    out.extend(kept.into_iter().map(str::to_string));
}

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
