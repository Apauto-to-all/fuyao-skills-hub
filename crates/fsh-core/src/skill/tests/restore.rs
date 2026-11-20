//! 恢复测试：归档副本移回 store、目标占用冲突、未归档拒绝。

use super::*;

#[test]
fn restore_moves_archive_back_to_store() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "back", "恢复"),
        None,
        false,
    )
    .unwrap();
    archive_key(&root(&install), "back", false).unwrap();

    restore(&root(&install), "back", false).unwrap();
    assert!(
        paths
            .skill_workdir("back")
            .join("references")
            .join("a.txt")
            .is_file(),
        "恢复是目录移动，内容不丢"
    );
    assert!(!paths.skill_archived("back").exists(), "归档副本随移动消失");
    // 恢复产生一条机器提交
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        repo.head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .message()
            .unwrap(),
        "restore(store): back"
    );
}

/// store 里同名键已占用：与归档侧占用同款 conflict，不做静默改名
#[test]
fn restore_rejects_when_store_side_occupied() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "dup", "占用"),
        None,
        false,
    )
    .unwrap();
    // 归档后 store 侧又出现同名目录（外部落盘），再试恢复：目标占用
    archive_key(&root(&install), "dup", false).unwrap();
    let work = paths.skill_workdir("dup");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();
    assert!(matches!(
        restore(&root(&install), "dup", false),
        Err(SkillError::Conflict { key }) if key == "dup"
    ));
    assert!(paths.skill_archived("dup").is_dir(), "拒绝时不动盘");
}

/// store 落点上有同名普通文件：同样拒绝覆盖。
/// 占位只认目录，该文件不是工作副本；但落点闸看存在性，落点被任何东西占着就拒绝
#[test]
fn restore_rejects_plain_file_on_store_side() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "clash", "store 侧有普通文件"),
        None,
        false,
    )
    .unwrap();
    archive_key(&root(&install), "clash", false).unwrap();
    let work = paths.skill_workdir("clash");
    std::fs::create_dir_all(paths.store()).unwrap();
    std::fs::write(&work, "不是目录\n").unwrap();

    assert!(matches!(
        restore(&root(&install), "clash", false),
        Err(SkillError::Conflict { key }) if key == "clash"
    ));
    assert_eq!(
        std::fs::read_to_string(&work).unwrap(),
        "不是目录\n",
        "拒绝时普通文件原样保留"
    );
    assert!(
        paths.skill_archived("clash").is_dir(),
        "拒绝时归档副本留在原处"
    );
}

/// 没有归档副本的键不可恢复：不存在的键报 skill_not_found
#[test]
fn restore_rejects_keys_without_archive_copy() {
    let (install, _paths) = setup();
    assert!(matches!(
        restore(&root(&install), "ghost", false),
        Err(SkillError::SkillNotFound { .. })
    ));
}
