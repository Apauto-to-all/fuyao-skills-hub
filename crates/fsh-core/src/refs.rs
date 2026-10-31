//! 引用集：一次走查项目登记簿得到的三类事实——引用对（谁在用哪版）、
//! 不可达项目（目录在但 lock 读不到，不贡献引用）、死记录（项目目录
//! 已不存在）。purge 守卫、GC 与 doctor 共用本口径，可达性语义全库唯一：
//! 不可达一律不贡献引用，由消费方各自告警处置。

use std::path::Path;

use crate::deploy;
use crate::hub;
use crate::paths::HubPaths;

/// 单条引用：某项目对某技能某版本的引用
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    /// 技能名
    pub skill: String,
    /// 精确版本
    pub version: String,
    /// 引用方项目路径（登记簿原样）
    pub project: String,
}

/// 不可达原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnreachableReason {
    /// skills.lock.toml 不存在（apply 必写 lock，缺文件即项目残缺）
    LockMissing,
    /// skills.lock.toml 存在但解析失败（文件损坏）
    LockCorrupted,
}

/// 不可达项目：目录在、lock 读不到；不贡献引用，消费方负责告警
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnreachableProject {
    /// 项目路径（登记簿原样）
    pub path: String,
    /// 不可达原因
    pub reason: UnreachableReason,
}

/// 死记录：项目目录已不存在；gc 按宽限判定后清理登记簿
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeadRecord {
    /// 项目路径（登记簿原样）
    pub path: String,
    /// 登记簿记录的最后 apply 时间（Unix 秒）
    pub last_applied: u64,
}

/// 引用集走查产出
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ReferenceSet {
    /// 全部可达项目的引用对并集
    pub references: Vec<Reference>,
    /// 不可达项目清单
    pub unreachable: Vec<UnreachableProject>,
    /// 死记录清单
    pub dead_records: Vec<DeadRecord>,
}

impl ReferenceSet {
    /// (skill, version) 是否被任一可达项目引用
    pub(crate) fn is_referenced(&self, skill: &str, version: &str) -> bool {
        self.references
            .iter()
            .any(|r| r.skill == skill && r.version == version)
    }

    /// 引用指定技能的项目路径清单（purge 守卫与 show 引用详情共用）
    pub(crate) fn referencing_projects(&self, skill: &str) -> Vec<String> {
        self.references
            .iter()
            .filter(|r| r.skill == skill)
            .map(|r| r.project.clone())
            .collect()
    }
}

/// 一次走查登记簿：目录不存在归死记录；lock 缺失或损坏归不可达；
/// 其余逐条收集引用对。hub 未初始化时按空登记簿处理（调用方前置校验）。
pub(crate) fn reference_set(paths: &HubPaths) -> Result<ReferenceSet, hub::HubError> {
    let registry = hub::load_registry(paths)?;
    let mut set = ReferenceSet::default();
    for rec in &registry.projects {
        let dir = Path::new(&rec.path);
        if !dir.is_dir() {
            set.dead_records.push(DeadRecord {
                path: rec.path.clone(),
                last_applied: rec.last_applied,
            });
            continue;
        }
        // lock 缺失不静默当空账本：apply 必写 lock，缺文件即项目残缺
        if !HubPaths::project_lock(dir).is_file() {
            set.unreachable.push(UnreachableProject {
                path: rec.path.clone(),
                reason: UnreachableReason::LockMissing,
            });
            continue;
        }
        match deploy::read_lock(dir) {
            Ok(lock) => {
                for e in &lock.entries {
                    set.references.push(Reference {
                        skill: e.name.clone(),
                        version: e.version.clone(),
                        project: rec.path.clone(),
                    });
                }
            }
            Err(_) => set.unreachable.push(UnreachableProject {
                path: rec.path.clone(),
                reason: UnreachableReason::LockCorrupted,
            }),
        }
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link;

    /// 建 hub + 登记一个项目（时间戳 1），返回 (install, paths, 项目)
    fn hub_with_registered_project() -> (tempfile::TempDir, HubPaths, tempfile::TempDir) {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        let proj = tempfile::tempdir().unwrap();
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(proj.path(), 1);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        (install, paths, proj)
    }

    /// 写项目 lock（一条 demo@1.0.0 引用）
    fn write_lock(proj: &Path, name: &str, version: &str) {
        let lock = deploy::LockFile {
            entries: vec![deploy::LockEntry {
                name: name.into(),
                version: version.into(),
                commit: "x".into(),
                tree_hash: "y".into(),
                kind: link::LinkKind::Junction,
                dst: format!(".fsh/skills/{name}"),
            }],
        };
        std::fs::write(
            HubPaths::project_lock(proj),
            toml::to_string(&lock).unwrap(),
        )
        .unwrap();
    }

    /// 读回登记簿里第一条记录的路径（登记簿按自身口径存路径，断言用它）
    fn registered_path(paths: &HubPaths) -> String {
        hub::load_registry(paths)
            .unwrap()
            .projects
            .first()
            .unwrap()
            .path
            .clone()
    }

    #[test]
    fn reachable_project_contributes_references() {
        let (install, paths, proj) = hub_with_registered_project();
        write_lock(proj.path(), "demo", "1.0.0");
        let set = reference_set(&paths).unwrap();
        assert_eq!(
            set.references,
            vec![Reference {
                skill: "demo".into(),
                version: "1.0.0".into(),
                project: registered_path(&paths),
            }]
        );
        assert!(set.unreachable.is_empty());
        assert!(set.dead_records.is_empty());
        let _ = install;
    }

    #[test]
    fn missing_lock_is_unreachable_and_contributes_nothing() {
        let (install, paths, _proj) = hub_with_registered_project();
        // 目录在、lock 缺失：不可达（原因 LockMissing），不产生引用
        let set = reference_set(&paths).unwrap();
        assert_eq!(
            set.unreachable,
            vec![UnreachableProject {
                path: registered_path(&paths),
                reason: UnreachableReason::LockMissing,
            }]
        );
        assert!(set.references.is_empty());
        assert!(set.dead_records.is_empty());
        let _ = install;
    }

    #[test]
    fn corrupted_lock_is_unreachable_not_dead() {
        let (install, paths, proj) = hub_with_registered_project();
        std::fs::write(HubPaths::project_lock(proj.path()), "not = [valid").unwrap();
        let set = reference_set(&paths).unwrap();
        assert_eq!(
            set.unreachable,
            vec![UnreachableProject {
                path: registered_path(&paths),
                reason: UnreachableReason::LockCorrupted,
            }]
        );
        assert!(set.references.is_empty());
        assert!(set.dead_records.is_empty(), "目录在就不是死记录");
        let _ = install;
    }

    #[test]
    fn vanished_dir_goes_to_dead_records_with_timestamp() {
        let (install, paths, _) = hub_with_registered_project();
        // 覆盖登记路径为不存在的目录
        let mut reg = hub::ProjectRegistry::default();
        reg.upsert(Path::new("Z:/definitely/not/here"), 12345);
        hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        let set = reference_set(&paths).unwrap();
        assert_eq!(
            set.dead_records,
            vec![DeadRecord {
                path: "Z:/definitely/not/here".into(),
                last_applied: 12345,
            }]
        );
        assert!(set.references.is_empty());
        assert!(set.unreachable.is_empty());
        let _ = install;
    }

    #[test]
    fn helpers_answer_reference_queries() {
        let (install, paths, proj) = hub_with_registered_project();
        write_lock(proj.path(), "demo", "1.0.0");
        let set = reference_set(&paths).unwrap();
        assert!(set.is_referenced("demo", "1.0.0"));
        assert!(!set.is_referenced("demo", "2.0.0"), "版本不同不算引用");
        assert!(!set.is_referenced("other", "1.0.0"));
        assert_eq!(
            set.referencing_projects("demo"),
            vec![registered_path(&paths)]
        );
        assert!(set.referencing_projects("other").is_empty());
        let _ = install;
    }

    #[test]
    fn empty_registry_yields_empty_set() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        let set = reference_set(&paths).unwrap();
        assert_eq!(set, ReferenceSet::default());
    }
}
