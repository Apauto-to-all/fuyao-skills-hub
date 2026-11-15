//! 自检的端到端断言：按检查项名取结论，验证三态与说明。

use std::fs;
use std::path::Path;

use super::{CheckStatus, DoctorCheck, DoctorReport, doctor};
use crate::hub;
use crate::paths::HubPaths;
use crate::project::{Targets, use_skill};

mod name_drift;

/// 按检查项名取结论：测试只关心某几项，其余不逐一断言
fn check<'a>(report: &'a DoctorReport, name: &str) -> &'a DoctorCheck {
    report
        .checks
        .iter()
        .find(|c| c.name == name)
        .expect("检查项应存在")
}

/// 内嵌表里全部内置技能的键：内置检查按内嵌表取值，测试与它同源
fn builtin_keys() -> Vec<String> {
    let skills = crate::bundled::skills();
    assert!(!skills.is_empty(), "skills 下应至少有一个内置技能");
    skills
        .iter()
        .map(|skill| crate::naming::builtin_key(&skill.name))
        .collect()
}

#[test]
fn fresh_hub_gets_ten_checks_with_expected_statuses() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let report = doctor(install.path()).unwrap();
    let names: Vec<&str> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "hub_writable",
            "cloud_path",
            "link_capability",
            "hub_integrity",
            "builtin_global",
            "builtin_content",
            "dead_projects",
            "orphan_destinations",
            "orphan_dirs",
            "name_drift",
        ]
    );
    // 全新 hub 的预期：可写、云盘、链接、装配、内置内容、无登记、
    // 无不可达、无垃圾都 ok；内置技能还没铺到全局是设计上就该提醒的一项
    for name in names {
        let expected = if name == "builtin_global" {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        };
        assert_eq!(check(&report, name).status, expected, "{name} 状态不符");
    }
}

#[test]
fn hub_integrity_fails_on_missing_artifact_and_corrupt_config() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
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
    // 缺件时不叠加「git 仓库打不开」：缺件就是原因，不再重复报
    assert!(
        !integrity.detail.contains("git 仓库打不开"),
        "缺件时不得叠加仓库打不开: {}",
        integrity.detail
    );

    // hub.toml 存在但语法坏：文件齐全也不等于可用
    fs::write(paths.gitignore(), hub::gitignore_content()).unwrap();
    fs::write(paths.hub_toml(), "user = ").unwrap();
    let report = doctor(install.path()).unwrap();
    let integrity = check(&report, "hub_integrity");
    assert_eq!(integrity.status, CheckStatus::Fail);
    assert!(
        integrity.detail.contains("hub.toml 解析失败"),
        "应说明解析失败: {}",
        integrity.detail
    );
    // 前缀只由错误给出，detail 里应当只出现一次
    assert_eq!(
        integrity.detail.matches("hub.toml 解析失败").count(),
        1,
        "前缀只应出现一次: {}",
        integrity.detail
    );
}

#[test]
fn hub_integrity_fails_on_unopenable_repository() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
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
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    // 忽略规则被清空：仓库仍可用，只是白名单失效、根级产物可能进提交
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
fn dead_project_path_is_warned() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
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
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    // 登记簿写到一半损坏：整份登记簿读不出，应由登记簿不可达项（⑧）告警
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
    // 路径注入构造云同步命中态：hub 根落在含 OneDrive 字样的路径下
    let base = tempfile::tempdir().unwrap();
    let install = base.path().join("OneDriveSync").join("hub");
    fs::create_dir_all(&install).unwrap();
    hub::init(&install, false).unwrap();
    let report = doctor(&install).unwrap();
    let cloud = check(&report, "cloud_path");
    assert_eq!(
        cloud.status,
        CheckStatus::Warn,
        "云同步路径应告警: {}",
        cloud.detail
    );
}

/// 孤儿落点：登记的落点在盘上已不存在时 WARN，只报不动
#[test]
fn orphan_destination_is_warned() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let mut reg = hub::ProjectRegistry::default();
    reg.record_use(
        &crate::project::Anchor::Project(project.path().to_path_buf()),
        hub::ProjectSkill {
            key: "demo".into(),
            agents: vec!["universal".to_string()],
            link: hub_scope_link(),
        },
        1,
    );
    hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "orphan_destinations");
    assert_eq!(item.status, CheckStatus::Warn);
}

/// 登记簿链接档位的取值辅助（孤儿落点测试夹具）
fn hub_scope_link() -> crate::link::LinkKind {
    crate::link::LinkKind::Junction
}

/// agent 清单读不出时不能给出假 OK：孤儿落点核对按「无法核对」告警
#[test]
fn unreadable_agent_list_makes_orphan_check_unverifiable() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    // 解析不了的 agent 配置：读名单即报错，落点就无从解析
    fs::write(paths.agents().join("broken.toml"), "skills_dir = \n").unwrap();

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "orphan_destinations");
    assert_eq!(item.status, CheckStatus::Warn, "{}", item.detail);
    assert!(
        item.detail.contains("无法核对"),
        "核对不了要点明无法核对: {}",
        item.detail
    );
}

/// 单条登记记录解析不出落点（点名了不存在的 agent）：同样按「无法核对」告警
#[test]
fn unresolvable_record_makes_orphan_check_unverifiable() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let mut reg = hub::ProjectRegistry::default();
    reg.record_use(
        &crate::project::Anchor::Project(project.path().to_path_buf()),
        hub::ProjectSkill {
            key: "demo".into(),
            // 名单里没有这个 agent：这条记录的落点解析必然失败
            agents: vec!["ghost".to_string()],
            link: hub_scope_link(),
        },
        1,
    );
    hub::save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "orphan_destinations");
    assert_eq!(item.status, CheckStatus::Warn, "{}", item.detail);
    assert!(
        item.detail.contains("无法核对"),
        "核对不了要点明无法核对: {}",
        item.detail
    );
}

/// 孤儿目录：store 下不含 SKILL.md 的残留目录 WARN，只报不动
#[test]
fn orphan_dir_is_warned() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    fs::create_dir_all(paths.store().join("residue")).unwrap();
    let report = doctor(install.path()).unwrap();
    let item = check(&report, "orphan_dirs");
    assert_eq!(item.status, CheckStatus::Warn);
}

/// 技能根下的子目录属于技能自身（如 references/），不算孤儿目录
#[test]
fn skill_subdirectory_is_not_an_orphan() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let skill = paths.store().join("demo");
    fs::create_dir_all(skill.join("references")).unwrap();
    fs::write(skill.join("SKILL.md"), "").unwrap();
    fs::write(skill.join("references/agents.md"), "# 附件\n").unwrap();
    let report = doctor(install.path()).unwrap();
    let item = check(&report, "orphan_dirs");
    assert_eq!(item.status, CheckStatus::Ok, "{}", item.detail);
}

/// 内置技能没进全局用法记录时告警并给出可直接照抄的命令骨架；
/// 真跑一次 use --global 之后这项不再是 WARN
#[test]
fn builtin_global_warns_with_command_skeleton_until_spread() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let keys = builtin_keys();

    let report = doctor(install.path()).unwrap();
    let item = check(&report, "builtin_global");
    assert_eq!(
        item.status,
        CheckStatus::Warn,
        "未铺全局应告警: {}",
        item.detail
    );
    // 每个内置技能都给出一条可照抄的骨架，agent 名留空由用户填
    for key in &keys {
        assert!(
            item.detail
                .contains(&format!("fsh use {key} --agent <你的 agent> --global")),
            "应给出 {key} 的可照抄命令骨架: {}",
            item.detail
        );
    }

    // 真实铺一次：agent 的全局目录指到临时目录，不碰真实家目录
    let global_dir = tempfile::tempdir().unwrap();
    fs::write(
        paths.agents().join("globalonly.toml"),
        format!(
            "global_skills_dir = {:?}\n",
            global_dir.path().display().to_string()
        ),
    )
    .unwrap();
    for key in &keys {
        use_skill(
            paths.root(),
            key,
            &["globalonly".to_string()],
            Targets::new(true, Vec::new()),
            false,
        )
        .unwrap();
    }

    let report = doctor(install.path()).unwrap();
    assert_eq!(
        check(&report, "builtin_global").status,
        CheckStatus::Ok,
        "已铺全局不应再告警"
    );
}

/// store 里的内置内容与内嵌一致时 OK；被改脏、目录整个缺失、出现孤儿目录
/// 都告警并指向 init 对齐；每次重新 init 之后回到 OK
#[test]
fn builtin_content_warns_on_drift_and_ok_after_init() {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let key = builtin_keys().remove(0);
    let work = paths.skill_workdir(&key);
    let report_of = || doctor(install.path()).unwrap();
    assert_eq!(
        check(&report_of(), "builtin_content").status,
        CheckStatus::Ok
    );

    // 内容被改脏：整份覆盖是 init 的动作，提示里带 copy 落点要重铺
    fs::write(work.join("SKILL.md"), "被改坏").unwrap();
    let report = report_of();
    let item = check(&report, "builtin_content");
    assert_eq!(
        item.status,
        CheckStatus::Warn,
        "内容不一致应告警: {}",
        item.detail
    );
    assert!(
        item.detail.contains("fsh init") && item.detail.contains("copy 落点需 unuse 后重铺"),
        "应指向 init 并说清 copy 落点: {}",
        item.detail
    );
    hub::init(install.path(), false).unwrap();
    assert_eq!(
        check(&report_of(), "builtin_content").status,
        CheckStatus::Ok
    );

    // 整个技能目录缺失：内容对不上，同样告警
    fs::remove_dir_all(&work).unwrap();
    assert_eq!(
        check(&report_of(), "builtin_content").status,
        CheckStatus::Warn
    );
    hub::init(install.path(), false).unwrap();
    assert_eq!(
        check(&report_of(), "builtin_content").status,
        CheckStatus::Ok
    );

    // 内嵌里已无的孤儿目录：init 会清掉，自检先点出来
    let stray = paths.store().join("builtin/stray");
    fs::create_dir_all(&stray).unwrap();
    fs::write(stray.join("SKILL.md"), "孤儿").unwrap();
    let report = report_of();
    let item = check(&report, "builtin_content");
    assert_eq!(item.status, CheckStatus::Warn);
    assert!(
        item.detail.contains("stray"),
        "应点名不一致的目录: {}",
        item.detail
    );
    hub::init(install.path(), false).unwrap();
    assert_eq!(
        check(&report_of(), "builtin_content").status,
        CheckStatus::Ok
    );
}
