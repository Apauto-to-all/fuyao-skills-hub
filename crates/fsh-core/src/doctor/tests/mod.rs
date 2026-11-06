//! 自检的端到端断言：按检查项名取结论，验证三态与说明。

use std::fs;
use std::path::Path;

use super::{CheckStatus, DoctorCheck, DoctorReport, doctor};
use crate::hub;
use crate::link;

/// 按检查项名取结论：测试只关心某几项，其余不逐一断言
fn check<'a>(report: &'a DoctorReport, name: &str) -> &'a DoctorCheck {
    report
        .checks
        .iter()
        .find(|c| c.name == name)
        .expect("检查项应存在")
}

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
            "hub_integrity",
            "exported_missing",
            "dead_projects",
            "reclaimable",
        ]
    );
    // 临时目录环境下的预期：可写 ok、云盘 ok、PF ok、链接 ok、装配 ok、
    // 无登记 ok、无不可达 ok、无垃圾 ok
    for name in names {
        assert_eq!(
            check(&report, name).status,
            CheckStatus::Ok,
            "{name} 应通过"
        );
    }
}

#[test]
fn hub_integrity_fails_on_missing_artifact_and_corrupt_config() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    // 缺 .gitignore：装配不完整，Fail 并点名缺件
    fs::remove_file(paths.gitignore()).unwrap();
    let report = doctor(install.path()).unwrap();
    let integrity = check(&report, "hub_integrity");
    assert_eq!(integrity.status, CheckStatus::Fail);
    assert!(
        integrity.detail.contains(".gitignore"),
        "应点名缺件: {}",
        integrity.detail
    );

    // hub.toml 存在但语法坏：文件齐全也不等于可用
    fs::write(paths.gitignore(), hub::GITIGNORE_CONTENT).unwrap();
    fs::write(paths.hub_toml(), "user = ").unwrap();
    let report = doctor(install.path()).unwrap();
    let integrity = check(&report, "hub_integrity");
    assert_eq!(integrity.status, CheckStatus::Fail);
    assert!(
        integrity.detail.contains("hub.toml 解析失败"),
        "应说明解析失败: {}",
        integrity.detail
    );
}

#[test]
fn hub_integrity_fails_on_unopenable_repository() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    // .git 在但打不开：装配齐全也救不了，必须报 Fail
    fs::remove_dir_all(paths.root().join(".git")).unwrap();
    fs::write(paths.root().join(".git"), "not a repository").unwrap();
    let report = doctor(install.path()).unwrap();
    let integrity = check(&report, "hub_integrity");
    assert_eq!(integrity.status, CheckStatus::Fail);
    assert!(
        integrity.detail.contains("git 仓库打不开"),
        "应说明仓库坏: {}",
        integrity.detail
    );
}

#[test]
fn hub_integrity_warns_on_rewritten_gitignore() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    // 忽略规则被清空：仓库仍可用，只是下次提交可能带进导出目录与日志
    fs::write(paths.gitignore(), "").unwrap();
    let report = doctor(install.path()).unwrap();
    let integrity = check(&report, "hub_integrity");
    assert_eq!(integrity.status, CheckStatus::Warn);
    assert!(
        integrity.detail.contains("白名单"),
        "应指向白名单: {}",
        integrity.detail
    );
}

#[test]
fn missing_export_is_reported() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    // 登记簿写死一个盘上不存在的导出目录
    let proj = tempfile::tempdir().unwrap();
    let mut reg = hub::ProjectRegistry::default();
    reg.upsert(proj.path(), 1);
    reg.projects[0].skills.push(hub::ProjectSkill {
        key: "demo".into(),
        version: Some("1.0.0".into()),
        agents: vec!["agents".into()],
        link: link::LinkKind::Junction,
    });
    hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

    let report = doctor(install.path()).unwrap();
    let exported = check(&report, "exported_missing");
    assert_eq!(exported.status, CheckStatus::Warn);
    assert!(exported.detail.contains("demo@1.0.0"));
}

#[test]
fn dead_project_path_is_warned() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    // 登记一个目录已不存在的项目：登记簿里的记录失效，应告警
    let mut reg = hub::ProjectRegistry::default();
    reg.upsert(Path::new("Z:/definitely/not/here"), 1);
    hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

    let report = doctor(install.path()).unwrap();
    let dead = check(&report, "dead_projects");
    assert_eq!(dead.status, CheckStatus::Warn);
    assert!(
        dead.detail.contains("not/here") || dead.detail.contains("not\\here"),
        "应指出路径: {}",
        dead.detail
    );
}

#[test]
fn unreadable_registry_is_warned() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    // 登记簿写到一半损坏：整份登记簿读不出，应由第七项告警
    fs::write(paths.projects_toml(), "projects = [").unwrap();

    let report = doctor(install.path()).unwrap();
    let dead = check(&report, "dead_projects");
    assert_eq!(dead.status, CheckStatus::Warn);
    assert!(
        dead.detail.contains("登记簿读不出"),
        "应说明读不出的原因: {}",
        dead.detail
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
    let cloud = check(&report, "cloud_path");
    assert_eq!(
        cloud.status,
        CheckStatus::Warn,
        "云同步路径应告警: {}",
        cloud.detail
    );
}

/// 仓库打不开时算不出可回收量：要报原因，不能印 0 装绿
#[test]
fn reclaimable_reports_reason_when_gc_fails() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    // 把 .git 换成同名文件：装配判定仍算齐备，但 git 仓库打不开
    fs::remove_dir_all(paths.root().join(".git")).unwrap();
    fs::write(paths.root().join(".git"), "not a repo").unwrap();
    let report = doctor(install.path()).unwrap();
    let item = check(&report, "reclaimable");
    assert_eq!(item.status, CheckStatus::Warn);
    assert!(
        item.detail.contains("统计失败"),
        "算不出时要报原因: {}",
        item.detail
    );
}
