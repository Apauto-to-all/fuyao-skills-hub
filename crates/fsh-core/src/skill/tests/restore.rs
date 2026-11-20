//! 恢复测试：归档副本移回 store、工作副本面已占用时跳过并报告、
//! 未归档的名字拒绝、整批一条提交。

use std::fs;

use super::*;
use crate::plan::ActionKind;

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

    restore_key(&root(&install), "back", false).unwrap();
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
        "restore(store): 根 1 个技能"
    );
}

/// store 里同名键已占用：预期内的不适用，跳过并报告，不做静默改名
#[test]
fn restore_skips_when_store_side_occupied() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "dup", "占用"),
        None,
        false,
    )
    .unwrap();
    // 归档后 store 侧又出现同名目录（外部落盘），再试恢复：跳过
    archive_key(&root(&install), "dup", false).unwrap();
    let work = paths.skill_workdir("dup");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    let outcome = restore_key(&root(&install), "dup", false).unwrap();
    assert!(outcome.applied.is_empty(), "占用的键不做: {outcome:?}");
    assert_eq!(outcome.skipped.len(), 1, "{outcome:?}");
    assert_eq!(outcome.skipped[0].key, "dup");
    assert_eq!(outcome.skipped[0].code, "conflict");
    assert_eq!(outcome.code(), 1, "一条都没做成时退出码为 1");
    assert!(paths.skill_archived("dup").is_dir(), "跳过时不动盘");
}

/// store 落点上有同名普通文件：同样跳过。
/// 占位只认目录，该文件不是工作副本；但落点闸看存在性，落点被任何东西占着就跳过
#[test]
fn restore_skips_plain_file_on_store_side() {
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
    fs::create_dir_all(paths.store()).unwrap();
    fs::write(&work, "不是目录\n").unwrap();

    let outcome = restore_key(&root(&install), "clash", false).unwrap();
    assert_eq!(outcome.skipped.len(), 1, "{outcome:?}");
    assert_eq!(outcome.skipped[0].code, "conflict");
    assert_eq!(
        fs::read_to_string(&work).unwrap(),
        "不是目录\n",
        "跳过时普通文件原样保留"
    );
    assert!(
        paths.skill_archived("clash").is_dir(),
        "跳过时归档副本留在原处"
    );
}

/// 没有归档副本的名字不可恢复：解析期就报 no_match，不落到「技能不存在」
#[test]
fn restore_rejects_names_that_are_not_archived() {
    let (install, _paths) = setup();
    let outcome = restore_key(&root(&install), "ghost", false);
    assert_eq!(
        outcome.unwrap_err().error_code(),
        "no_match",
        "未归档的名字属于整批不入的错误"
    );
}

/// 一整套里只有 store 侧占用的那条跳过：其余照常恢复，整批只留一条提交，
/// 提交消息带整套名与条数
#[test]
fn restore_takes_the_whole_set_and_skips_the_occupied_one() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "held"] {
        add_key(
            &root(&install),
            &make_skill(src_dir.path(), name, "整套恢复"),
            Some("k1/k2"),
            false,
        )
        .unwrap();
    }
    archive(&root(&install), &Selector::all(Some("k1/k2".into())), false).unwrap();
    // 归档后 store 侧又出现同名目录：恢复时跳过这一条
    let held = paths.skill_workdir("k1/k2/held");
    fs::create_dir_all(&held).unwrap();
    fs::write(held.join("SKILL.md"), "# 占位\n").unwrap();

    let outcome = restore(&root(&install), &Selector::all(Some("k1/k2".into())), false).unwrap();
    assert_eq!(
        outcome.applied,
        vec!["k1/k2/a".to_string(), "k1/k2/b".to_string()],
        "整套里能做的全做掉: {outcome:?}"
    );
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].code, "conflict");
    assert_eq!(outcome.code(), 0, "做成了一条就不算失败");

    assert!(paths.skill_workdir("k1/k2/a").is_dir());
    assert!(paths.skill_workdir("k1/k2/b").is_dir());
    assert!(
        paths.skill_archived("k1/k2/held").is_dir(),
        "被占用的留在归档面"
    );

    // 整批一条提交：消息含整套名与条数
    let repo = git2::Repository::open(paths.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.message().unwrap(), "restore(store): k1/k2 2 个技能");
    assert!(!git::is_dirty(&repo).unwrap());
}

/// 预演：逐条列出移动与跳过项，盘上一字未动、也没有提交
#[test]
fn restore_dry_run_lists_moves_and_skips_without_touching_disk() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    for name in ["a", "held"] {
        add_key(
            &root(&install),
            &make_skill(src_dir.path(), name, "恢复预演"),
            Some("k1"),
            false,
        )
        .unwrap();
    }
    archive(&root(&install), &Selector::all(Some("k1".into())), false).unwrap();
    let held = paths.skill_workdir("k1/held");
    fs::create_dir_all(&held).unwrap();
    fs::write(held.join("SKILL.md"), "# 占位\n").unwrap();
    let before = git2::Repository::open(paths.root())
        .unwrap()
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .id();

    let outcome = restore(&root(&install), &Selector::all(Some("k1".into())), true).unwrap();
    assert!(outcome.dry_run);
    assert_eq!(outcome.applied, vec!["k1/a".to_string()]);
    assert_eq!(outcome.skipped.len(), 1);
    let moves: Vec<_> = outcome
        .actions
        .iter()
        .filter(|step| step.action == ActionKind::Move)
        .collect();
    assert_eq!(moves.len(), 1, "只有没被占用的一条要动: {outcome:?}");
    assert_eq!(moves[0].from.as_deref(), Some("archive/k1/a"));
    assert_eq!(moves[0].to.as_deref(), Some("store/k1/a"));
    // 脏 hub 的手改快照另算：计划里属于恢复自己的提交只有一条
    let restores: Vec<_> = outcome
        .actions
        .iter()
        .filter(|step| step.target.as_deref() == Some("restore(store): k1 1 个技能"))
        .collect();
    assert_eq!(restores.len(), 1, "整批只压一条提交: {outcome:?}");

    assert!(paths.skill_archived("k1/a").is_dir(), "预演不得动归档面");
    assert!(
        !paths.skill_workdir("k1/a").exists(),
        "预演不得把工作副本移回来"
    );
    let now = git2::Repository::open(paths.root())
        .unwrap()
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .id();
    assert_eq!(before, now, "预演不产生提交");
}
