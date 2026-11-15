//! 落点测试：锚点解析（作用域分派、查表、去重、末段拼名）、父目录占用、落点计数。

use std::fs;
use std::path::PathBuf;

use super::*;
use crate::ErrorCode;
use crate::agents::{AgentInfo, AgentSource};
use crate::project::dest::dirs_equal;

#[test]
fn destination_count_aggregates_registry_usage() {
    // 落点解析只认 agent 名单里的键：用内置的 universal，不依赖清单顺序
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let mut registry = crate::hub::ProjectRegistry::default();
    // 项目目录要活到断言之后：registry 里存的是路径字符串
    let projects: Vec<_> = (0..2).map(|_| temp_project()).collect();
    for project in &projects {
        registry.record_use(
            &crate::project::Anchor::Project(project.to_path_buf()),
            crate::hub::ProjectSkill {
                key: "demo".into(),
                agents: vec!["universal".to_string()],
                link: link::LinkKind::Junction,
            },
            0,
        );
    }
    // 落点解析收口在 project 模块：doctor 孤儿落点核对用同一份
    hub::save_registry(&paths, &registry, &crate::lock::write_lock(&paths).unwrap()).unwrap();
    assert_eq!(
        crate::project::registered_destinations(&paths)
            .unwrap()
            .len(),
        2
    );
}

/// 项目锚点：落点 = 项目根 + agent 的技能目录 + 键末段
#[test]
fn project_anchor_joins_skills_dir_and_key_tail() {
    let (_install, paths) = setup_with_skill();
    let known = crate::agents::list(&paths).unwrap();
    let project = temp_project();

    let dests = Anchor::Project(project.clone())
        .resolve(&known, &["universal".to_string()], "mattpocock/skills/tdd")
        .unwrap();

    assert_eq!(dests.len(), 1);
    assert_eq!(dests[0].path, project.join(".agents/skills/tdd"));
    assert_eq!(dests[0].agents, vec!["universal".to_string()]);
}

/// 两个 agent 共用同一个技能目录：合并成一条落点，一次使用只建一个入口
#[test]
fn shared_skills_dir_collapses_into_one_destination() {
    let (_install, paths) = setup_with_skill();
    let known = crate::agents::list(&paths).unwrap();
    let project = temp_project();
    let keys = vec!["universal".to_string(), "codex".to_string()];

    let dests = Anchor::Project(project.clone())
        .resolve(&known, &keys, "demo")
        .unwrap();

    assert_eq!(dests.len(), 1, "共用目录只留一条落点");
    assert_eq!(dests[0].path, project.join(".agents/skills/demo"));
    assert_eq!(dests[0].agents, keys);
}

/// 全局落点取加载时展开好的绝对目录：解析这一步不再碰配置原文
#[test]
fn global_anchor_uses_the_dir_expanded_at_load() {
    // 原文与展开结果刻意不一致：解析若回头再展开，得到的就是家目录下的那个
    let known = vec![AgentInfo {
        key: "probe".to_string(),
        display_name: "probe".to_string(),
        skills_dir: None,
        global_skills_dir: Some("~/.never-expanded/skills".to_string()),
        global_dir: Some(PathBuf::from("D:/expanded/skills")),
        source: AgentSource::User,
    }];

    let dests = Anchor::Global
        .resolve(&known, &["probe".to_string()], "demo")
        .unwrap();

    assert_eq!(dests[0].path, PathBuf::from("D:/expanded/skills/demo"));
}

/// 全局落点从配置原文在家目录下展开：展开发生在加载，解析只拼键末段
#[test]
fn global_destination_is_expanded_against_home() {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    fs::create_dir_all(paths.agents()).unwrap();
    fs::write(
        paths.agents().join("tilde-global.toml"),
        "global_skills_dir = \"~/.fsh-probe/skills\"\n",
    )
    .unwrap();
    let known = crate::agents::list(&paths).unwrap();

    let dests = Anchor::Global
        .resolve(&known, &["tilde-global".to_string()], "demo/skill")
        .unwrap();

    assert_eq!(dests.len(), 1);
    let expected = std::env::home_dir()
        .unwrap()
        .join(".fsh-probe/skills")
        .join("skill");
    assert_eq!(
        dests[0].path, expected,
        "全局落点必须是家目录展开后的绝对路径"
    );
}

#[test]
fn dirs_equal_reports_error_when_a_side_is_unreadable() {
    // 任一侧读不了即报错，不静默判成「一致」——一致会被当成「可以安全撤回」
    let ok = tempfile::tempdir().unwrap();
    let gone = ok.path().join("no-such-subdir");
    let err = dirs_equal(ok.path(), &gone).unwrap_err();
    assert_eq!(err.error_code(), "io", "读不了目录应上抛 IO 错误");
}

#[test]
fn dirs_equal_compares_content() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    fs::write(a.path().join("x.txt"), "same").unwrap();
    fs::write(b.path().join("x.txt"), "same").unwrap();
    assert!(dirs_equal(a.path(), b.path()).unwrap());

    fs::write(b.path().join("x.txt"), "diff").unwrap();
    assert!(!dirs_equal(a.path(), b.path()).unwrap());
}
