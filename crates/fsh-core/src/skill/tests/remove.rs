//! 删除测试：两侧副本哪侧在场删哪侧，被引用时跳过并报告，整批一条提交。

use std::fs;

use crate::plan::{ActionKind, PlanStep};

use super::*;

/// 当前 HEAD 的提交 id：断言整批只多一条提交用
fn head_id(paths: &HubPaths) -> git2::Oid {
    git2::Repository::open(paths.root())
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap()
}

#[test]
fn remove_skips_referenced_skill_and_reports_it() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "dead", "待删"),
        None,
        false,
    )
    .unwrap();
    // 项目在用 → 跳过并报告，盘上不动
    let proj = tempfile::tempdir().unwrap();
    register_usage(&paths, proj.path(), "dead");
    let outcome = remove_key(&root(&install), "dead", false).unwrap();
    assert!(outcome.applied.is_empty(), "被引用的技能不删: {outcome:?}");
    assert_eq!(outcome.skipped.len(), 1, "{outcome:?}");
    assert_eq!(outcome.skipped[0].key, "dead");
    assert_eq!(outcome.skipped[0].code, "referenced");
    assert_eq!(outcome.code(), 1, "一条都没做成时退出码为 1");
    assert!(paths.skill_workdir("dead").is_dir(), "跳过时不动盘");

    // 登记簿清掉引用后删除成功：一条提交，历史仍在
    hub::save_registry(
        &paths,
        &hub::ProjectRegistry::default(),
        &crate::lock::write_lock(&paths).unwrap(),
    )
    .unwrap();
    let outcome = remove_key(&root(&install), "dead", false).unwrap();
    assert_eq!(outcome.applied, vec!["dead".to_string()]);
    assert_eq!(outcome.code(), 0);
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
    // 再删同名 → 该前缀下没有这个技能
    assert_eq!(
        remove_key(&root(&install), "dead", false)
            .unwrap_err()
            .error_code(),
        "no_match"
    );
}

/// 删除归档副本：提交消息的目标面是归档面，与工作副本面区分；
/// 同一键在各面删除时消息各自点名被删的那一面
#[test]
fn remove_of_archived_skill_commits_archive_face() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "cold", "归档待删"),
        None,
        false,
    )
    .unwrap();
    archive_key(&root(&install), "cold", false).unwrap();

    let outcome = remove_key(&root(&install), "cold", false).unwrap();
    assert_eq!(outcome.applied, vec!["cold".to_string()]);
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "remove(archive): cold");
    assert!(!paths.skill_archived("cold").exists());
    assert!(repo.head().is_ok(), "hub 仓库历史应完好");
}

#[test]
fn remove_is_skipped_while_a_global_usage_holds_the_skill() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "held", "全局在用"),
        None,
        false,
    )
    .unwrap();
    register_global_usage(&paths, "held", &["dsh", "universal"]);

    // 全局段只算一条引用：agent 去重排序后一并列进跳过原因
    let outcome = remove_key(&root(&install), "held", false).unwrap();
    assert!(outcome.applied.is_empty(), "全局在用时不做: {outcome:?}");
    assert_eq!(outcome.skipped[0].code, "referenced");
    let message = &outcome.skipped[0].message;
    assert!(message.contains("dsh"), "跳过原因点出全局 agent: {message}");
    assert!(message.contains("universal"), "{message}");
    // 跳过时不动盘
    assert!(paths.skill_workdir("held").is_dir());
}

/// 两侧都有副本：store 与 archive 的副本要一起删掉，提交里两个面都点名
#[test]
fn remove_deletes_both_copies_when_both_exist() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "twin", "两侧都有"),
        None,
        false,
    )
    .unwrap();
    archive_key(&root(&install), "twin", false).unwrap();
    // 归档后 store 侧又出现同名工作副本（外部落盘）：两个面各有一份
    let work = paths.skill_workdir("twin");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    let outcome = remove_key(&root(&install), "twin", false).unwrap();
    assert_eq!(outcome.applied, vec!["twin".to_string()]);
    assert!(!paths.skill_workdir("twin").is_dir(), "工作副本要删掉");
    assert!(!paths.skill_archived("twin").is_dir(), "归档副本也要删掉");
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        repo.head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .message()
            .unwrap(),
        "remove(store,archive): twin",
        "一次操作一条提交，两个删除面都点名"
    );
}

/// 两侧都有副本时预演列出两份删除，提交只有一步：删除面按实际删到的面用逗号连接。
/// 手工放回的工作副本让仓库变脏，动盘前的手改快照因此排在清单最前
#[test]
fn remove_dry_run_lists_both_copies() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "twin-dry", "两侧都有的预演"),
        None,
        false,
    )
    .unwrap();
    archive_key(&root(&install), "twin-dry", false).unwrap();
    let work = paths.skill_workdir("twin-dry");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    let out = remove_key(&root(&install), "twin-dry", true).unwrap();
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
    assert!(paths.skill_workdir("twin-dry").is_dir(), "预演不动盘");
    assert!(paths.skill_archived("twin-dry").is_dir(), "预演不动盘");
}

/// 一整套里只有被引用的那条跳过：其余照常删，整批只留一条提交，
/// 提交消息带整套名与条数
#[test]
fn remove_takes_the_whole_set_and_skips_the_referenced_one() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "held"] {
        add_key(
            &root(&install),
            &make_skill(src_dir.path(), name, "整套删除"),
            Some("k1/k2"),
            false,
        )
        .unwrap();
    }
    register_global_usage(&paths, "k1/k2/held", &["dsh"]);
    let before = head_id(&paths);

    let outcome = remove(&root(&install), &Selector::all(Some("k1/k2".into())), false).unwrap();
    assert_eq!(
        outcome.applied,
        vec!["k1/k2/a".to_string(), "k1/k2/b".to_string()],
        "整套里能删的全删掉: {outcome:?}"
    );
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].code, "referenced");
    assert_eq!(outcome.code(), 0, "做成了一条就不算失败");

    assert!(!paths.skill_workdir("k1/k2/a").exists());
    assert!(!paths.skill_workdir("k1/k2/b").exists());
    assert!(
        paths.skill_workdir("k1/k2/held").is_dir(),
        "被引用的留在原处"
    );

    // 整批一条提交：消息含整套名与条数
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "remove(store): k1/k2 2 个技能");
    assert_eq!(head.parent(0).unwrap().id(), before, "整批只产生一条提交");
    assert!(!git::is_dirty(&repo).unwrap());
}

/// 选中集里一个技能都没有：计划里没有删除步也没有提交步，盘上什么都不发生
#[test]
fn remove_of_an_empty_set_does_nothing() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "elsewhere", "别处"),
        Some("k1"),
        false,
    )
    .unwrap();
    let before = head_id(&paths);

    let outcome = remove(
        &root(&install),
        &Selector::all(Some("k1/empty".into())),
        false,
    )
    .unwrap();
    assert!(outcome.applied.is_empty(), "{outcome:?}");
    assert!(outcome.skipped.is_empty(), "{outcome:?}");
    assert_eq!(outcome.code(), 0, "什么都没做不算失败");
    assert_eq!(head_id(&paths), before, "没有提交");
    assert!(paths.skill_workdir("k1/elsewhere").is_dir(), "别套不动");
}

#[test]
fn remove_of_nested_key_spares_sibling_namespace() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let b = make_skill(src_dir.path(), "b-skill", "兄弟甲");
    add_key(&root(&install), &b, Some("a"), false).unwrap();
    let c = make_skill(src_dir.path(), "c-skill", "兄弟乙");
    add_key(&root(&install), &c, Some("a"), false).unwrap();

    remove(
        &root(&install),
        &Selector::named(Some("a".into()), vec!["b-skill".into()]),
        false,
    )
    .unwrap();
    assert!(!paths.skill_workdir("a/b-skill").exists());
    assert!(
        paths.skill_workdir("a/c-skill").join("SKILL.md").is_file(),
        "删除不得误删嵌套兄弟键"
    );
}
