//! 一条龙初始化、幂等重跑、预演与半截装配修复。

use std::fs;

use super::assert_initialized;
use crate::git;
use crate::hub::{
    GITIGNORE_CONTENT, HubConfig, HubError, hub_init, init, load_config, save_config,
};
use crate::paths::HubPaths;

#[test]
fn hub_init_creates_full_layout_and_first_commit() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub_init(install.path()).unwrap();
    assert_initialized(&paths);
}

#[test]
fn init_first_run_initializes_hub() {
    let install = tempfile::tempdir().unwrap();
    let outcome = init(install.path(), false).unwrap();
    assert!(outcome.initialized, "首建应报告 initialized");
    assert!(!outcome.already);
    assert_initialized(&HubPaths::from_root(install.path()));
}

#[test]
fn init_rerun_is_idempotent_and_touches_nothing() {
    let install = tempfile::tempdir().unwrap();
    init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let before = fs::read_to_string(paths.hub_toml()).unwrap();
    let outcome = init(install.path(), false).unwrap();
    assert!(
        !outcome.initialized && outcome.already,
        "幂等重跑应只报告 already"
    );
    assert_eq!(
        fs::read_to_string(paths.hub_toml()).unwrap(),
        before,
        "幂等重跑不得改动现有数据"
    );
}

#[test]
fn init_dry_run_lists_actions_and_writes_nothing() {
    let install = tempfile::tempdir().unwrap();
    let outcome = init(install.path(), true).unwrap();
    assert!(outcome.dry_run && !outcome.already);
    assert_eq!(
        outcome.actions,
        vec![
            "create store/ export/ agents/",
            "write .gitignore hub.toml",
            "git init + first commit",
        ]
    );
    assert_eq!(
        fs::read_dir(install.path()).unwrap().count(),
        0,
        "预演不得落盘"
    );
}

#[test]
fn init_dry_run_on_initialized_hub_reports_already() {
    let install = tempfile::tempdir().unwrap();
    init(install.path(), false).unwrap();
    let outcome = init(install.path(), true).unwrap();
    assert!(
        outcome.dry_run && outcome.already,
        "已建的预演应报告 already"
    );
    assert!(outcome.actions.is_empty());
}

#[test]
fn hub_init_rejects_existing_hub_without_damage() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub_init(install.path()).unwrap();
    // 人为改动 hub.toml 后再次 init：拒绝且原数据不动
    let cfg = load_config(&paths).unwrap().unwrap();
    let mut modified = cfg.clone();
    modified.user.name = "someone".to_string();
    save_config(&paths, &modified).unwrap();
    assert!(matches!(
        hub_init(install.path()),
        Err(HubError::AlreadyHub(_))
    ));
    let reloaded = load_config(&paths).unwrap().unwrap();
    assert_eq!(reloaded.user.name, "someone", "重复 init 不得破坏现有数据");
}

#[test]
fn init_repairs_half_assembled_hub() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub_init(install.path()).unwrap();
    // 半截装配：仓库在，配置与忽略规则被删
    fs::remove_file(paths.gitignore()).unwrap();
    fs::remove_file(paths.hub_toml()).unwrap();
    assert!(!paths.is_initialized());

    let outcome = init(install.path(), false).unwrap();
    assert!(outcome.initialized, "补齐缺件应报告 initialized");
    assert!(!outcome.already);
    assert_eq!(outcome.repaired, vec![".gitignore", "hub.toml"]);
    assert_initialized(&paths);
}

#[test]
fn init_rewrites_drifted_gitignore_and_commits() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub_init(install.path()).unwrap();
    fs::write(paths.gitignore(), "logs/\n").unwrap();
    // 内容坏不算缺件：判据只查存在，这里仍判为已初始化
    assert!(paths.is_initialized());

    let outcome = init(install.path(), false).unwrap();
    assert_eq!(outcome.repaired, vec![".gitignore"]);
    assert_eq!(
        fs::read_to_string(paths.gitignore()).unwrap(),
        GITIGNORE_CONTENT
    );
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(!git::is_dirty(&repo).unwrap(), "修复应提交进仓库");
}

#[test]
fn init_dry_run_plans_repair_for_half_assembled_hub() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub_init(install.path()).unwrap();
    fs::remove_file(paths.gitignore()).unwrap();
    let outcome = init(install.path(), true).unwrap();
    assert!(outcome.dry_run && !outcome.already);
    assert!(
        outcome.actions.iter().any(|a| a.contains("repair hub")),
        "预演应列出修复动作: {:?}",
        outcome.actions
    );
    assert!(!paths.gitignore().exists(), "预演不得落盘");
}

/// 仓库被删但配置还在：重建仓库时沿用原有身份，不覆盖 hub.toml
#[test]
fn hub_init_keeps_config_when_repository_is_gone() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub_init(install.path()).unwrap();
    let mut cfg = HubConfig::v1();
    cfg.user.name = "alice".to_string();
    save_config(&paths, &cfg).unwrap();
    fs::remove_dir_all(paths.root().join(".git")).unwrap();
    assert!(!paths.is_initialized());

    hub_init(install.path()).unwrap();
    assert_eq!(load_config(&paths).unwrap().unwrap().user.name, "alice");
    // 首提交身份用沿用下来的配置
    let repo = git2::Repository::open(paths.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.author().name().unwrap(), "alice");
}

/// 修复不得覆盖用户数据：hub.toml 存在时原样保留
#[test]
fn init_repair_keeps_existing_config() {
    let install = tempfile::tempdir().unwrap();
    let paths = hub_init(install.path()).unwrap();
    let mut cfg = HubConfig::v1();
    cfg.user.name = "alice".to_string();
    save_config(&paths, &cfg).unwrap();
    fs::remove_file(paths.gitignore()).unwrap();

    let outcome = init(install.path(), false).unwrap();
    assert_eq!(outcome.repaired, vec![".gitignore"]);
    assert_eq!(load_config(&paths).unwrap().unwrap().user.name, "alice");
    // 修复只提交补回来的文件：用户未提交的 hub.toml 改动仍留在工作区
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(
        git::is_dirty(&repo).unwrap(),
        "修复不得把用户未提交的改动卷进提交"
    );
}
