//! 一次读盘的结果：登记簿、agent 清单、内置表、两面目录、登记落点各取一次。

use std::fs;
use std::path::Path;

use crate::doctor::scan::scan;
use crate::identity::{path_identity, project_key};
use crate::paths::HubPaths;

/// 全新 hub：登记簿读得出且为空，落点为空集，store / archive 两面都走过
#[test]
fn scan_collects_every_fact_once() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());

    let scanned = scan(paths);

    assert!(
        scanned.registry_error.is_none(),
        "{:?}",
        scanned.registry_error
    );
    assert!(!scanned.builtin.is_empty(), "内置技能表应随扫描带出");
    let names: Vec<&str> = scanned.faces.iter().map(|face| face.name).collect();
    assert_eq!(names, ["store", "archive"], "两面各走一遍，顺序固定");
    assert!(scanned.destinations.unwrap().is_empty(), "空登记簿没有落点");
}

/// 登记落点带 agent 归属：消费方直接拿到路径与 agent，不必把路径转成字符串再解回来
#[test]
fn scan_carries_destinations_with_agent_ownership() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let mut registry = crate::hub::ProjectRegistry::default();
    registry.record_use(
        &crate::project::Anchor::Project(project.path().to_path_buf()),
        crate::hub::ProjectSkill {
            key: "demo".into(),
            agents: vec!["universal".into()],
            link: crate::link::LinkKind::Junction,
        },
        1,
    );
    crate::hub::save_registry(&paths, &registry, &crate::lock::write_lock(&paths).unwrap())
        .unwrap();

    let scanned = scan(paths);

    let dests = scanned.destinations.unwrap();
    assert_eq!(dests.len(), 1);
    assert_eq!(
        dests[0].agents,
        vec!["universal".to_string()],
        "归属随落点带出"
    );
    // 登记簿以规范化绝对路径作表键：临时目录带 8.3 短名时，进表那一刻已展开成长名
    let expected = Path::new(&project_key(project.path())).join(".agents/skills/demo");
    assert_eq!(
        path_identity(&dests[0].path),
        path_identity(&expected),
        "落点仍是路径原值，不经字符串往返"
    );
}

/// 登记簿读不出：原因随扫描带出一次，落点直接判为核对不了——两项说同一句话
#[test]
fn scan_records_registry_error_and_leaves_destinations_unresolved() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    fs::write(paths.projects_toml(), "projects = [").unwrap();

    let scanned = scan(paths);

    let error = scanned.registry_error.expect("坏登记簿应带出原因");
    let unresolved = scanned
        .destinations
        .expect_err("读不出登记簿就核对不了落点");
    assert_eq!(unresolved, error, "同一次读取，两项说同一句话");
}
