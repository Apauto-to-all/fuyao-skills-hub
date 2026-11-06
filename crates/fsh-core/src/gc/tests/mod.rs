//! gc 的端到端断言：真建 hub 与 git 仓库，验证清扫动作、宽限期与半截 hub 拒绝。

use super::*;
use crate::git as git_mod;
use crate::link;

/// 建一个带单版本技能的 hub：工作副本 + tag + 手工导出，无任何项目引用
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
        &paths.exported("demo", "1.0.0"),
    )
    .unwrap();
    (install, paths)
}

/// 在登记簿里登记一个项目，并声明它引用了某技能某版本
fn registry_referencing(project: &Path, key: &str, version: Option<&str>) -> hub::ProjectRegistry {
    let mut reg = hub::ProjectRegistry::default();
    reg.upsert(project, 1);
    reg.projects[0].skills.push(hub::ProjectSkill {
        key: key.into(),
        version: version.map(str::to_string),
        agents: vec!["agents".into()],
        link: link::LinkKind::Junction,
    });
    reg
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

#[test]
fn gc_grace_keeps_fresh_unreferenced_dir() {
    let (install, paths) = hub_with_skill();
    // 宽限期拉满：刚导出的目录（mtime=现在）必须被留下
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
    assert!(paths.exported("demo", "1.0.0").exists(), "目录应保留");
    let _ = install;
}

#[test]
fn gc_walks_nested_keys_and_cleans_orphans() {
    let (install, paths) = hub_with_skill();
    let repo = git2::Repository::open(paths.root()).unwrap();
    // 嵌套键技能：工作副本 + tag + 手工导出，三层目录都在 export 下
    let wd = paths.skill_workdir("anthropics/skills/pdf");
    fs::create_dir_all(&wd).unwrap();
    fs::write(wd.join("SKILL.md"), "# pdf\n").unwrap();
    git_mod::commit_all(&repo, "add pdf", "fuyao", "fuyao@local").unwrap();
    git_mod::tag_version(&repo, "anthropics/skills/pdf", "1.0.0").unwrap();
    git_mod::materialize(
        &repo,
        "anthropics/skills/pdf/v1.0.0",
        "store/anthropics/skills/pdf",
        &paths.exported("anthropics/skills/pdf", "1.0.0"),
    )
    .unwrap();
    // 嵌套键下的孤儿版本目录（无 tag）：递归收集应找到并清掉
    let orphan = paths
        .export()
        .join("anthropics")
        .join("skills")
        .join("pdf")
        .join("9.9.9");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(orphan.join("x.txt"), "x").unwrap();

    let report = gc(paths.root(), false).unwrap();
    assert!(!orphan.exists(), "嵌套键下的孤儿导出目录应被清掉");
    assert!(
        report
            .items
            .iter()
            .any(|i| matches!(i, GcAction::OrphanCleaned { .. })),
        "孤儿清理应有动作记录: {:?}",
        report.items
    );
    // 有 tag 且内容一致的嵌套导出目录：按无引用正常清，不再残留
    assert!(
        !paths.exported("anthropics/skills/pdf", "1.0.0").exists(),
        "嵌套键的导出目录应被递归收集后按规则清掉"
    );
    assert!(
        report
            .items
            .iter()
            .any(|i| matches!(i, GcAction::Cleaned { .. })),
        "嵌套键导出目录应记为 Cleaned: {:?}",
        report.items
    );
    let _ = install;
}

#[test]
fn gc_rejects_hub_without_full_assembly() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    fs::remove_file(paths.hub_toml()).unwrap();
    // 装配不完整：给出稳定错误码，而不是 git2 的原始错误
    match gc(paths.root(), true) {
        Err(GcError::HubNotInitialized(p)) => {
            assert_eq!(p, paths.root().display().to_string())
        }
        Err(other) => panic!("错误类型不对: {other}"),
        Ok(_) => panic!("半截 hub 不得被 gc 接受"),
    }
}
