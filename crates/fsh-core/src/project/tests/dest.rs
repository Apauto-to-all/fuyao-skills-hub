//! 落点与导出守卫测试：父目录占用、导出目录一致性、落点计数。

use std::fs;

use super::*;
use crate::ErrorCode;
use crate::project::dest::{dirs_equal, resolve_global};

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

/// 全局落点从配置原文展开：`~` 写法要落到家目录下，不能把字面量当路径
#[test]
fn global_destination_expands_tilde_config_at_resolve_time() {
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

    let dests = resolve_global(&known, &["tilde-global".to_string()], "demo/skill").unwrap();
    assert_eq!(dests.len(), 1);
    let expected = std::env::home_dir()
        .unwrap()
        .join(".fsh-probe/skills")
        .join("skill");
    assert_eq!(
        dests[0].path,
        expected.display().to_string(),
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
