//! 一条龙初始化、幂等重跑、预演与半截装配修复。

use std::fs;

use super::{assert_initialized, head_oid, setup, setup_with_empty_builtin};
use crate::git;
use crate::hub::{HubConfig, gitignore_content, init, load_config};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep};

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
            PlanStep::of(ActionKind::Create, "agents/"),
            PlanStep::of(ActionKind::Create, "store/"),
            PlanStep::of(ActionKind::Create, "archive/"),
            PlanStep::of(ActionKind::Write, ".gitignore"),
            PlanStep::of(ActionKind::Write, "hub.toml"),
            PlanStep::of(ActionKind::Init, "git"),
            // 首建写下跟踪面后有一次提交，随后才轮到内置技能对齐
            PlanStep::commit("init(hub): hub"),
            PlanStep::of(ActionKind::Write, "store/builtin/fuyao-skills-hub"),
            PlanStep::commit("init(builtin): builtin"),
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

/// 已建的 hub 上用户改过配置：重跑 init 只报告 already、不报错、数据一字不动
#[test]
fn init_rerun_reports_already_and_keeps_data() {
    let (install, paths) = setup();
    let mut modified = load_config(&paths).unwrap().unwrap();
    modified.schema_version = 99;
    fs::write(paths.hub_toml(), modified.to_toml().unwrap()).unwrap();

    let outcome = init(install.path(), false).unwrap();
    assert!(
        !outcome.initialized && outcome.already,
        "重跑应只报告 already"
    );
    let reloaded = load_config(&paths).unwrap().unwrap();
    assert_eq!(reloaded.schema_version, 99, "重复 init 不得破坏现有数据");
}

#[test]
fn init_repairs_half_assembled_hub() {
    let (install, paths) = setup();
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
    let (install, paths) = setup();
    fs::write(paths.gitignore(), "logs/\n").unwrap();
    // 内容坏不算缺件：判据只查存在，这里仍判为已初始化
    assert!(paths.is_initialized());

    let outcome = init(install.path(), false).unwrap();
    assert_eq!(outcome.repaired, vec![".gitignore"]);
    assert_eq!(
        fs::read_to_string(paths.gitignore()).unwrap(),
        gitignore_content()
    );
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(!git::is_dirty(&repo).unwrap(), "修复应提交进仓库");
}

/// 预演与实跑出自同一份计划：清单里的每一步真跑都落地，真跑的每一步清单里都有
#[test]
fn init_dry_run_matches_what_repair_does() {
    let (install, paths) = setup();
    fs::remove_dir_all(paths.archive()).unwrap();
    fs::remove_file(paths.gitignore()).unwrap();

    let planned = init(install.path(), true).unwrap().actions;
    assert_eq!(
        planned,
        vec![
            PlanStep::of(ActionKind::Create, "archive/"),
            PlanStep::of(ActionKind::Repair, ".gitignore"),
            PlanStep::commit("repair(hub): hub"),
        ],
        "预演清单按装配计划列"
    );
    assert!(!paths.archive().exists(), "预演不得落盘");
    assert!(!paths.gitignore().exists(), "预演不得落盘");

    let outcome = init(install.path(), false).unwrap();
    assert_eq!(outcome.repaired, vec![".gitignore"]);
    assert!(paths.archive().is_dir(), "预演列了建目录，真跑就该建出来");
    assert_eq!(
        fs::read_to_string(paths.gitignore()).unwrap(),
        gitignore_content()
    );
}

/// 只补目录不算补缺件：目录不是缺件清单里的一项，结果载荷只报 already
#[test]
fn init_restores_missing_directories_without_reporting_parts() {
    let (install, paths) = setup();
    for dir in [paths.store(), paths.archive(), paths.agents()] {
        fs::remove_dir_all(&dir).unwrap();
    }

    let outcome = init(install.path(), false).unwrap();
    assert!(outcome.already, "只补目录时结果载荷只报 already");
    assert!(outcome.repaired.is_empty());
    for dir in [paths.store(), paths.archive(), paths.agents()] {
        assert!(dir.is_dir(), "目录应由 init 建回来: {dir:?}");
    }
}

/// 装配的任何盘上改动都在取锁之后：锁被占时一个目录都不建
#[test]
fn init_touches_nothing_when_lock_is_held() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    let _held = crate::lock::write_lock(&paths).unwrap();

    let err = init(install.path(), false).unwrap_err();
    assert_eq!(crate::ErrorCode::error_code(&err), "lock_held");
    for dir in [paths.store(), paths.archive(), paths.agents()] {
        assert!(!dir.exists(), "取锁失败不得先建目录: {dir:?}");
    }
    assert!(!paths.gitignore().exists(), "取锁失败不得写 .gitignore");
}

/// 仓库被删但配置还在：重建仓库时沿用原有配置，不覆盖 hub.toml
#[test]
fn init_keeps_config_when_repository_is_gone() {
    let (install, paths) = setup();
    let mut cfg = HubConfig::v1();
    cfg.schema_version = 99;
    fs::write(paths.hub_toml(), cfg.to_toml().unwrap()).unwrap();
    fs::remove_dir_all(paths.root().join(".git")).unwrap();
    assert!(!paths.is_initialized());

    init(install.path(), false).unwrap();
    assert_eq!(load_config(&paths).unwrap().unwrap().schema_version, 99);
    // 首提交是机器提交：固定身份与配置无关
    let repo = git2::Repository::open(paths.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.author().name().unwrap(), "fsh");
}

/// 修复不得覆盖用户数据：hub.toml 存在时原样保留
#[test]
fn init_repair_keeps_existing_config() {
    let (install, paths) = setup();
    let mut cfg = HubConfig::v1();
    cfg.schema_version = 99;
    fs::write(paths.hub_toml(), cfg.to_toml().unwrap()).unwrap();
    fs::remove_file(paths.gitignore()).unwrap();

    let outcome = init(install.path(), false).unwrap();
    assert_eq!(outcome.repaired, vec![".gitignore"]);
    assert_eq!(load_config(&paths).unwrap().unwrap().schema_version, 99);
    // 修复落盘后产生一条机器提交，工作区干净
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(!git::is_dirty(&repo).unwrap());
    assert_eq!(
        repo.head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .message()
            .unwrap(),
        "repair(hub): hub"
    );
}

#[test]
fn init_lands_embedded_builtin_skill_with_commit() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    let outcome = init(install.path(), false).unwrap();
    assert!(outcome.initialized, "首建应报告 initialized");

    let skill = crate::bundled::skills()
        .into_iter()
        .find(|s| s.name == "fuyao-skills-hub")
        .expect("仓库应内置 fuyao-skills-hub");
    let work = paths.skill_workdir("builtin/fuyao-skills-hub");
    for file in &skill.files {
        assert_eq!(
            fs::read(work.join(&file.path)).unwrap(),
            file.contents,
            "{} 应与内嵌内容逐字节一致",
            file.path
        );
    }
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(!git::is_dirty(&repo).unwrap(), "内置技能应随 init 提交");
}

#[test]
fn init_second_run_keeps_builtin_bytes_and_makes_no_commit() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    init(install.path(), false).unwrap();
    let work_md = paths
        .skill_workdir("builtin/fuyao-skills-hub")
        .join("SKILL.md");
    let before = fs::read(&work_md).unwrap();
    let head_before = head_oid(&paths);

    let outcome = init(install.path(), false).unwrap();
    assert!(!outcome.initialized && outcome.already);
    assert_eq!(
        fs::read(&work_md).unwrap(),
        before,
        "内容一致应一个字节不动"
    );
    assert_eq!(head_oid(&paths), head_before, "内容一致不得产生提交");
}

#[test]
fn init_dry_run_lists_builtin_alignment_actions() {
    let (install, paths) = setup_with_empty_builtin();
    let outcome = init(install.path(), true).unwrap();
    assert!(outcome.dry_run && outcome.already);
    assert!(
        outcome.actions.contains(&PlanStep::of(
            ActionKind::Write,
            "store/builtin/fuyao-skills-hub"
        )),
        "已初始化 hub 的预演应列出内置技能写盘动作: {:?}",
        outcome.actions
    );
    assert!(!paths.store().join("builtin").exists(), "预演不得落盘");
}

#[test]
fn init_restores_removed_builtin_skill() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    let key = "builtin/fuyao-skills-hub";
    init(install.path(), false).unwrap();
    let work_md = paths.skill_workdir(key).join("SKILL.md");
    let embedded = fs::read(&work_md).unwrap();

    // 移出后 init 还原：工作副本整份回来
    crate::skill::remove(install.path(), key, false).unwrap();
    assert!(!work_md.exists());
    init(install.path(), false).unwrap();
    assert_eq!(fs::read(&work_md).unwrap(), embedded);
}
