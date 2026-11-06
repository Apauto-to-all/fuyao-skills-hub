//! 项目登记簿的读写与引用判定。

use std::path::Path;

use crate::hub::{ProjectRegistry, ProjectSkill, load_registry, save_registry};
use crate::link::LinkKind;
use crate::paths::HubPaths;

#[test]
fn save_registry_accepts_held_token() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::new(install.path());
    // 凭证随取随用：持锁写登记簿成功，语句结束凭证即释放
    let reg = ProjectRegistry::default();
    save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
    assert_eq!(load_registry(&paths).unwrap(), ProjectRegistry::default());
}

#[test]
fn registry_roundtrip_and_upsert() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::new(install.path());
    // 缺失文件 = 空登记簿
    assert_eq!(load_registry(&paths).unwrap(), ProjectRegistry::default());

    let mut reg = ProjectRegistry::default();
    reg.upsert(install.path(), 100);
    reg.upsert(install.path(), 200); // 同路径 upsert 只刷新时间戳
    save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
    let back = load_registry(&paths).unwrap();
    assert_eq!(back.projects.len(), 1);
    assert_eq!(back.projects[0].last_used, 200);
}

/// 引用判定只读登记簿：技能列表里出现键即算被引用
#[test]
fn referencing_projects_reads_skill_list() {
    let mut reg = ProjectRegistry::default();
    reg.upsert(Path::new("P:/a"), 1);
    reg.upsert(Path::new("P:/b"), 2);
    reg.projects[0].skills.push(ProjectSkill {
        key: "demo".into(),
        version: None,
        agents: vec!["universal".into()],
        link: LinkKind::Junction,
    });
    let refs = reg.referencing_projects("demo");
    assert_eq!(refs.len(), 1, "只列出真正引用的项目");
    assert!(refs[0].ends_with("a") || refs[0].ends_with("b"));
    assert!(reg.referencing_projects("other").is_empty());
}
