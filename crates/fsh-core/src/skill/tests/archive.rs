//! 归档测试：工作副本撤走而版本与导出目录保留。

use std::fs;

use super::*;

#[test]
fn archive_is_rejected_while_a_global_usage_holds_the_skill() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "arch", None, "全局在用的归档"),
        None,
        false,
    )
    .unwrap();
    register_global_usage(&paths, "arch", &["dsh"]);

    // 只有全局用法也算被引用：拒绝并点出占用它的 agent
    match archive(&root(&install), "arch", false) {
        Err(SkillError::Referenced {
            key,
            projects,
            global_agents,
        }) => {
            assert_eq!(key, "arch");
            assert!(projects.is_empty(), "没有项目在用: {projects:?}");
            assert_eq!(global_agents, vec!["dsh".to_string()]);
        }
        other => panic!("全局在用时必须拒绝归档: {other:?}"),
    }
    assert!(paths.skill_workdir("arch").exists(), "拒绝时不动盘");
}

#[test]
fn archive_removes_workdir_but_keeps_versions_and_export() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "arch", None, "归档供版"),
        None,
        false,
    )
    .unwrap();
    // 先造一个导出目录，归档要连它一起收
    let exported = paths.exported("arch", "0.1.0");
    fs::create_dir_all(&exported).unwrap();
    fs::write(exported.join("SKILL.md"), "# arch\n").unwrap();

    archive(&root(&install), "arch", false).unwrap();
    // 编辑面与导出目录删除后，该键只剩下 tag 与 git 历史
    assert!(!paths.skill_workdir("arch").exists());
    assert!(!paths.export().join("arch").exists());
    assert!(!paths.root().join("meta").exists());
    // 版本链全在、历史提交可查
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "arch").unwrap(), vec!["0.1.0"]);
    assert!(!git::is_dirty(&repo).unwrap());
}
