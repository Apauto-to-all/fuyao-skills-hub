//! 删除测试：两侧副本哪侧在场删哪侧，被引用时拒绝。

use crate::plan::{ActionKind, PlanStep};

use super::*;

#[test]
fn remove_deletes_workdir_and_rejects_when_referenced() {
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
    register_usage(&paths, proj.path(), "dead");
    assert!(matches!(remove(&root(&install), "dead", false),
        Err(SkillError::Referenced { key, .. }) if key == "dead"));
    // 登记簿清掉引用后删除成功：提交历史仍在
    hub::save_registry(
        &paths,
        &hub::ProjectRegistry::default(),
        &crate::lock::write_lock(&paths).unwrap(),
    )
    .unwrap();
    remove(&root(&install), "dead", false).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(!paths.skill_workdir("dead").exists());
    assert_eq!(
        repo.head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .message()
            .unwrap(),
        "remove(store): dead"
    );
    assert!(repo.head().is_ok(), "hub 仓库历史应完好");
    // 再删同名 → 不存在
    assert!(matches!(
        remove(&root(&install), "dead", false),
        Err(SkillError::SkillNotFound { .. })
    ));
}

/// 删除归档副本：提交消息的目标面是归档面，与工作副本面区分；
/// 同一键在各面删除时消息各自点名被删的那一面
#[test]
fn remove_of_archived_skill_commits_archive_face() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "cold", None, "归档待删"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "cold", false).unwrap();

    remove(&root(&install), "cold", false).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "remove(archive): cold");
    assert!(!paths.skill_archived("cold").exists());
    assert!(repo.head().is_ok(), "hub 仓库历史应完好");
}

#[test]
fn remove_is_rejected_while_a_global_usage_holds_the_skill() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "held", None, "全局在用"),
        None,
        false,
    )
    .unwrap();
    register_global_usage(&paths, "held", &["dsh", "universal"]);

    // 全局段只算一条引用：agent 去重排序后一并列出
    match remove(&root(&install), "held", false) {
        Err(SkillError::Referenced {
            key,
            projects,
            global_agents,
        }) => {
            assert_eq!(key, "held");
            assert!(projects.is_empty(), "没有项目在用: {projects:?}");
            assert_eq!(
                global_agents,
                vec!["dsh".to_string(), "universal".to_string()]
            );
        }
        other => panic!("全局在用时必须拒绝删除: {other:?}"),
    }
    // 拒绝时不动盘
    assert!(paths.skill_workdir("held").is_dir());
}

/// 两侧都有副本：store 与 archive 的副本要一起删掉
#[test]
fn remove_deletes_both_copies_when_both_exist() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "twin", None, "两侧都有"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "twin", false).unwrap();
    // 归档后 store 侧又出现同名工作副本（外部落盘）：两个面各有一份
    let work = paths.skill_workdir("twin");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    remove(&root(&install), "twin", false).unwrap();
    assert!(!paths.skill_workdir("twin").is_dir(), "工作副本要删掉");
    assert!(!paths.skill_archived("twin").is_dir(), "归档副本也要删掉");
}

/// 两侧都有副本时预演列出两份删除，提交只有一步：删除面按实际删到的面用逗号连接。
/// 手工放回的工作副本让仓库变脏，动盘前的手改快照因此排在清单最前
#[test]
fn remove_dry_run_lists_both_copies() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "twin-dry", None, "两侧都有的预演"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "twin-dry", false).unwrap();
    let work = paths.skill_workdir("twin-dry");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    let out = remove(&root(&install), "twin-dry", true).unwrap();
    assert_eq!(
        out.actions,
        vec![
            PlanStep::commit("sync(store): manual"),
            PlanStep::of(ActionKind::Remove, "store/twin-dry"),
            PlanStep::of(ActionKind::Remove, "archive/twin-dry"),
            PlanStep::commit("remove(store,archive): twin-dry"),
        ],
        "两份删除共一条提交，目标面用逗号连接"
    );
}

/// 两侧都有副本时实跑只产出一条机器提交，消息里两个面都点名
#[test]
fn remove_of_both_copies_commits_one_message_naming_both_faces() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "twin-commit", None, "两侧都提交"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "twin-commit", false).unwrap();
    let work = paths.skill_workdir("twin-commit");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    remove(&root(&install), "twin-commit", false).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(
        head.message().unwrap(),
        "remove(store,archive): twin-commit",
        "一次操作一条提交，两个删除面都点名"
    );
    assert!(head.parent(0).is_ok(), "提交历史里没有第二条删除提交");
    assert!(!paths.skill_workdir("twin-commit").is_dir());
    assert!(!paths.skill_archived("twin-commit").is_dir());
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
    assert!(!paths.skill_workdir("a/b-skill").exists());
    assert!(
        paths.skill_workdir("a/c-skill").join("SKILL.md").is_file(),
        "删除不得误删嵌套兄弟键"
    );
}
