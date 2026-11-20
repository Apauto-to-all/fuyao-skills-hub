//! 一条龙初始化、幂等重跑、预演与半截装配修复。

use std::fs;
use std::path::Path;

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

/// 预演清单与实跑同源：清单里的每一步真跑都落地——建目录的建出、写盘的写出、
/// 提交的进历史。清单由计划投影，实跑按同一份计划落盘
#[test]
fn init_dry_run_steps_all_land_in_the_real_run() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    let planned = init(install.path(), true).unwrap().actions;
    assert_eq!(
        planned.last().map(|step| step.action),
        Some(ActionKind::Commit),
        "提交步排在最后：装配与内置对齐都落完才提交"
    );
    assert_eq!(
        install.path().read_dir().unwrap().count(),
        0,
        "预演不得落盘"
    );

    init(install.path(), false).unwrap();
    let subjects = commit_subjects(&paths);
    for step in &planned {
        let target = step.target.as_deref().expect("计划里的每一步都写对象");
        match step.action {
            ActionKind::Create => {
                assert!(
                    install.path().join(target).is_dir(),
                    "建目录没落地: {target}"
                )
            }
            ActionKind::Write => {
                assert!(install.path().join(target).exists(), "写盘没落地: {target}")
            }
            ActionKind::Init => assert!(install.path().join(".git").is_dir(), "仓库没建出"),
            ActionKind::Commit => assert!(
                subjects.contains(&target.to_string()),
                "提交没进历史: {target}"
            ),
            other => panic!("首建不该出现 {other:?} 步"),
        }
    }
    assert!(
        planned.contains(&PlanStep::of(
            ActionKind::Write,
            "store/builtin/fuyao-skills-hub"
        )),
        "内置技能对齐要排进同一份计划: {planned:?}"
    );
}

/// 一趟 init 只提一条提交：装配与内置对齐在同一份计划里落库，
/// 首条提交就把内置技能一并收进仓库
#[test]
fn init_fresh_run_commits_assembly_and_builtin_once() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    init(install.path(), false).unwrap();

    assert_eq!(
        commit_subjects(&paths),
        vec!["init(hub): hub"],
        "一次 init 只提一条提交"
    );
    let repo = git2::Repository::open(paths.root()).unwrap();
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(
        tree.get_path(Path::new("store/builtin/fuyao-skills-hub/SKILL.md"))
            .is_ok(),
        "内置技能应随同一次提交入库"
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

/// 半截装配：预演列出的提交步与实跑真正产生的提交逐字一致——手改快照排在最前，
/// 修复提交在后；列出的盘面动作真跑全部落地
#[test]
fn init_dry_run_on_half_assembled_hub_matches_the_real_run() {
    let (install, paths) = setup();
    fs::remove_dir_all(paths.archive()).unwrap();
    fs::remove_file(paths.hub_toml()).unwrap();
    let before = commit_subjects(&paths).len();

    let planned = init(install.path(), true).unwrap().actions;
    assert!(!paths.archive().exists(), "预演不得落盘");
    assert!(!paths.hub_toml().exists(), "预演不得落盘");
    let planned_commits: Vec<String> = planned
        .iter()
        .filter(|step| step.action == ActionKind::Commit)
        .filter_map(|step| step.target.clone())
        .collect();
    assert_eq!(
        planned_commits,
        ["sync(store): manual", "repair(hub): hub"],
        "预演应列出封存手改与修复两条提交: {planned:?}"
    );

    let outcome = init(install.path(), false).unwrap();
    assert_eq!(outcome.repaired, vec!["hub.toml"]);
    let produced: Vec<String> = commit_subjects(&paths)[before..].to_vec();
    assert_eq!(
        planned_commits, produced,
        "预演列出的提交步要与实跑产生的提交逐字一致"
    );
    assert!(paths.archive().is_dir(), "预演列了建目录，真跑就该建出来");
    assert!(paths.hub_toml().exists(), "预演列了补件，真跑就该补回来");
}

/// already 只有一个来源：gitignore 漂移属要补的缺件，预演与实跑都不得报 already
#[test]
fn init_dry_run_and_real_run_agree_on_already_for_drift() {
    let (install, paths) = setup();
    fs::write(paths.gitignore(), "logs/\n").unwrap();

    let dry = init(install.path(), true).unwrap();
    assert!(!dry.already, "要补缺件时预演不得报 already: {dry:?}");
    assert!(
        dry.actions
            .contains(&PlanStep::of(ActionKind::Repair, ".gitignore")),
        "预演应列出修复步: {:?}",
        dry.actions
    );

    let real = init(install.path(), false).unwrap();
    assert!(!real.already, "要补缺件时实跑不得报 already");
    assert_eq!(real.repaired, vec![".gitignore"]);
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

/// 装配无变更、内置技能漂移：先封存手改，再由一条提交把对齐结果入库
#[test]
fn init_aligns_drifted_builtin_with_one_commit() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    init(install.path(), false).unwrap();
    fs::write(
        paths
            .skill_workdir("builtin/fuyao-skills-hub")
            .join("SKILL.md"),
        "被改坏",
    )
    .unwrap();
    let before = commit_subjects(&paths).len();

    init(install.path(), false).unwrap();
    let produced: Vec<String> = commit_subjects(&paths)[before..].to_vec();
    assert_eq!(
        produced,
        ["sync(store): manual", "init(builtin): builtin"],
        "手改封存在前，对齐提交在后且只有一条"
    );
}

/// hub 仓库的机器提交消息，从旧到新
fn commit_subjects(paths: &HubPaths) -> Vec<String> {
    let repo = git2::Repository::open(paths.root()).unwrap();
    let mut walk = repo.revwalk().unwrap();
    walk.push_head().unwrap();
    let mut subjects: Vec<String> = walk
        .map(|oid| {
            repo.find_commit(oid.unwrap())
                .unwrap()
                .message()
                .unwrap()
                .trim_end()
                .to_string()
        })
        .collect();
    subjects.reverse();
    subjects
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
    let selector = crate::select::Selector::named(
        Some(crate::naming::BUILTIN_PREFIX.to_string()),
        vec!["fuyao-skills-hub".to_string()],
    );
    crate::skill::remove(install.path(), &selector, false).unwrap();
    assert!(!work_md.exists());
    init(install.path(), false).unwrap();
    assert_eq!(fs::read(&work_md).unwrap(), embedded);
}
