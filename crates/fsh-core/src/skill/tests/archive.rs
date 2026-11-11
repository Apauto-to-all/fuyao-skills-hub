//! 归档测试：被引用拒绝、目录移动、目标占用冲突。

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

/// 归档侧同名键已占用：统一 conflict，不做静默改名
#[test]
fn archive_rejects_when_archive_side_occupied() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "arch", None, "归档侧占用"),
        None,
        false,
    )
    .unwrap();
    let dst = paths.skill_archived("arch");
    fs::create_dir_all(&dst).unwrap();
    fs::write(dst.join("SKILL.md"), "# 占位\n").unwrap();

    assert!(matches!(
        archive(&root(&install), "arch", false),
        Err(SkillError::Conflict { key }) if key == "arch"
    ));
    assert!(paths.skill_workdir("arch").is_dir(), "拒绝时不动盘");
}

#[test]
fn archive_removes_workdir_and_commits() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "arch", None, "归档供版"),
        None,
        false,
    )
    .unwrap();

    archive(&root(&install), "arch", false).unwrap();
    assert!(!paths.skill_workdir("arch").exists());
    // 归档副本整目录移入归档面：内容逐字节保留
    assert!(
        paths
            .skill_archived("arch")
            .join("references")
            .join("a.txt")
            .is_file(),
        "归档是目录移动，内容不丢"
    );
    assert!(!paths.root().join("meta").exists());
    // 归档产生一条机器提交，历史可查
    let repo = git2::Repository::open(paths.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.message().unwrap(), "archive(store): arch");
    assert!(!git::is_dirty(&repo).unwrap());
    assert!(fs::read_dir(paths.store()).is_ok());
}
