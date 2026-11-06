//! 删除测试：标签与工作副本一并删除，被引用时拒绝。

use super::*;

#[test]
fn remove_deletes_tags_and_rejects_when_referenced() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "dead", None, "待删"),
        None,
        false,
    )
    .unwrap();
    // 项目在用 → 拒绝
    let proj = tempfile::tempdir().unwrap();
    register_usage(&paths, proj.path(), "dead", None);
    assert!(matches!(remove(&root(&install), "dead", false),
        Err(SkillError::Referenced { key, .. }) if key == "dead"));
    // 登记簿清掉引用后删除成功：tag 清空、提交历史仍在
    hub::save_registry(
        &paths,
        &hub::ProjectRegistry::default(),
        &crate::lock::write_lock(&paths).unwrap(),
    )
    .unwrap();
    remove(&root(&install), "dead", false).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(git::tag_list(&repo, "dead").unwrap().is_empty());
    assert!(repo.head().is_ok(), "hub 仓库历史应完好");
    // 再删同名 → 不存在
    assert!(matches!(
        remove(&root(&install), "dead", false),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn remove_of_nested_key_spares_sibling_namespace() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let b = make_skill(src_dir.path(), "b-skill", None, "兄弟甲");
    add(&root(&install), &b, Some("a"), false).unwrap();
    let c = make_skill(src_dir.path(), "c-skill", None, "兄弟乙");
    add(&root(&install), &c, Some("a"), false).unwrap();

    remove(&root(&install), "a/b-skill", false).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(git::tag_list(&repo, "a/b-skill").unwrap().is_empty());
    assert_eq!(
        git::tag_list(&repo, "a/c-skill").unwrap(),
        vec!["0.1.0"],
        "删除不得误删嵌套兄弟键的 tag"
    );
}
