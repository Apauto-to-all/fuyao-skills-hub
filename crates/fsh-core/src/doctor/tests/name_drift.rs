//! 键末段与 frontmatter name 一致性检查的断言：两面扫描，漂移 WARN 只报不动。

use std::fs;
use std::path::Path;

use super::{CheckStatus, check, doctor};
use crate::paths::HubPaths;

/// 造一个技能目录：目录名与 frontmatter name 由调用方给定，可先一致再改
fn write_skill(dir: &Path, fm_name: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {fm_name}\ndescription: 测试技能\n---\n\n# {fm_name}\n正文\n"),
    )
    .unwrap();
}

/// 手改 frontmatter name 制造漂移：store 面报 WARN，文案含键、实际 name 与修复指引
#[test]
fn store_edited_name_drift_is_warned_with_fix_hint() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let skill = paths.store().join("demo");
    write_skill(&skill, "demo");
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: other\ndescription: 测试技能\n---\n\n# other\n正文\n",
    )
    .unwrap();

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "name_drift");
    assert_eq!(item.status, CheckStatus::Warn, "{}", item.detail);
    assert!(
        item.detail.contains("store/demo") && item.detail.contains("name=other"),
        "应点名键与实际 name: {}",
        item.detail
    );
    assert!(
        item.detail.contains("整目录改名")
            && item.detail.contains("重新 add")
            && item.detail.contains("把 name 改回"),
        "应给出修复指引: {}",
        item.detail
    );
}

/// 手挪目录制造漂移：frontmatter name 还叫旧名、目录已换新名，store 面报 WARN
#[test]
fn store_moved_dir_drift_is_warned() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    write_skill(&paths.store().join("demo"), "demo");
    fs::rename(paths.store().join("demo"), paths.store().join("moved")).unwrap();

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "name_drift");
    assert_eq!(item.status, CheckStatus::Warn, "{}", item.detail);
    assert!(
        item.detail.contains("store/moved") && item.detail.contains("name=demo"),
        "应点名新键与残留的 name: {}",
        item.detail
    );
}

/// archive 面同样检查、同样报法：漂移键带面名前缀报 WARN
#[test]
fn archive_name_drift_is_warned() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    write_skill(&paths.archive().join("demo"), "other");

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "name_drift");
    assert_eq!(item.status, CheckStatus::Warn, "{}", item.detail);
    assert!(
        item.detail.contains("archive/demo") && item.detail.contains("name=other"),
        "应点名 archive 面的键与实际 name: {}",
        item.detail
    );
}

/// 两面全部一致时不产生任何漂移输出项：OK 说明里不带任何键
#[test]
fn consistent_names_report_no_drift() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    write_skill(&paths.store().join("demo"), "demo");
    write_skill(&paths.store().join("group").join("nested"), "nested");
    write_skill(&paths.archive().join("old"), "old");

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "name_drift");
    assert_eq!(item.status, CheckStatus::Ok, "{}", item.detail);
    assert_eq!(item.detail, "键末段与 frontmatter name 全部一致");
}
