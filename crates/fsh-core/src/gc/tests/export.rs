//! 导出目录清扫：引用保留、无引用清理、脏目录跳过、全局引用不误删、预演不动盘。

use super::*;

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
fn gc_keeps_exports_referenced_only_by_a_global_usage() {
    let (install, paths) = hub_with_skill();
    // 全局段记一条引用：导出目录不得被回收
    let mut reg = hub::ProjectRegistry::default();
    reg.global_mut().skills.push(hub::ProjectSkill {
        key: "demo".into(),
        version: Some("1.0.0".into()),
        agents: vec!["dsh".into()],
        link: link::LinkKind::Junction,
    });
    hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

    let report = gc(paths.root(), false).unwrap();
    assert!(
        paths.exported("demo", "1.0.0").exists(),
        "全局在用版本不得回收: {:?}",
        report.items
    );
    // 全局记录不是项目死记录，清扫后必须还在
    assert_eq!(hub::load_registry(&paths).unwrap().globals.skills.len(), 1);
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
