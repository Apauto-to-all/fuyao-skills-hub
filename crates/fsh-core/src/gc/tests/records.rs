//! 登记簿死记录与 hub 装配：宽限期、嵌套键遍历、半截 hub 拒绝。

use super::*;

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
