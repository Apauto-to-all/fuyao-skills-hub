//! 归档测试：被引用跳过并报告、整批一条提交、目录移动、目标占用冲突。

use std::fs;

use super::*;

/// 被引用的技能跳过并报告，其余照常归档；一条都没做成就报非零退出码
#[test]
fn archive_skips_a_skill_held_by_a_global_usage() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "arch", "全局在用的归档"),
        None,
        false,
    )
    .unwrap();
    register_global_usage(&paths, "arch", &["dsh"]);

    let outcome = archive_key(&root(&install), "arch", false).unwrap();
    assert!(outcome.applied.is_empty(), "被引用的技能不做: {outcome:?}");
    assert_eq!(outcome.skipped.len(), 1, "{outcome:?}");
    assert_eq!(outcome.skipped[0].key, "arch");
    assert_eq!(outcome.skipped[0].code, "referenced");
    assert!(
        outcome.skipped[0].message.contains("dsh"),
        "跳过原因点出占用它的 agent: {}",
        outcome.skipped[0].message
    );
    assert_eq!(outcome.code(), 1, "一条都没做成时退出码为 1");
    assert!(paths.skill_workdir("arch").exists(), "跳过时不动盘");
    assert!(!paths.skill_archived("arch").exists());
}

/// 一整套里只有被引用的那条跳过：其余照常归档，整批只留一条提交，
/// 提交消息带整套名与条数
#[test]
fn archive_takes_the_whole_set_and_skips_the_referenced_one() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    for name in ["a", "b", "held"] {
        add_key(
            &root(&install),
            &make_skill(src_dir.path(), name, "整套归档"),
            Some("k1/k2"),
            false,
        )
        .unwrap();
    }
    register_global_usage(&paths, "k1/k2/held", &["dsh"]);

    let outcome = archive(&root(&install), &Selector::all(Some("k1/k2".into())), false).unwrap();
    assert_eq!(
        outcome.applied,
        vec!["k1/k2/a".to_string(), "k1/k2/b".to_string()],
        "整套里能做的全做掉: {outcome:?}"
    );
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].code, "referenced");
    assert_eq!(outcome.code(), 0, "做成了一条就不算失败");

    assert!(paths.skill_archived("k1/k2/a").is_dir());
    assert!(paths.skill_archived("k1/k2/b").is_dir());
    assert!(
        paths.skill_workdir("k1/k2/held").is_dir(),
        "被引用的留在原处"
    );

    // 整批一条提交：消息含整套名与条数
    let repo = git2::Repository::open(paths.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.message().unwrap(), "archive(store): k1/k2 2 个技能");
    assert!(!git::is_dirty(&repo).unwrap());
}

/// 归档侧同名键已占用：统一 conflict，不做静默改名
#[test]
fn archive_rejects_when_archive_side_occupied() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "arch", "归档侧占用"),
        None,
        false,
    )
    .unwrap();
    let dst = paths.skill_archived("arch");
    fs::create_dir_all(&dst).unwrap();
    fs::write(dst.join("SKILL.md"), "# 占位\n").unwrap();

    assert!(matches!(
        archive_key(&root(&install), "arch", false),
        Err(SkillError::Conflict { key }) if key == "arch"
    ));
    assert!(paths.skill_workdir("arch").is_dir(), "拒绝时不动盘");
}

/// 两侧都有副本：归档副本算归档侧占用，与移除同一口径
#[test]
fn archive_rejects_when_both_copies_exist() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "twin", "两侧都有的归档"),
        None,
        false,
    )
    .unwrap();
    archive_key(&root(&install), "twin", false).unwrap();
    // 手工把归档副本拷回 store：两个面各有一份
    let work = paths.skill_workdir("twin");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# 占位\n").unwrap();

    assert!(matches!(
        archive_key(&root(&install), "twin", false),
        Err(SkillError::Conflict { key }) if key == "twin"
    ));
    assert!(paths.skill_workdir("twin").is_dir(), "拒绝时不动盘");
    assert!(paths.skill_archived("twin").is_dir(), "拒绝时不动盘");
}

/// 归档落点上有同名普通文件：拒绝覆盖。
/// 占位只认目录，所以该文件不构成「已归档副本」；但落点闸看的是存在性，
/// 落点上任何东西都算占用——静默替换会把用户文件从工作区抹掉
#[test]
fn archive_rejects_plain_file_on_archive_side() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "clash", "归档侧有普通文件"),
        None,
        false,
    )
    .unwrap();
    let dst = paths.skill_archived("clash");
    fs::create_dir_all(paths.archive()).unwrap();
    fs::write(&dst, "不是目录\n").unwrap();

    assert!(matches!(
        archive_key(&root(&install), "clash", false),
        Err(SkillError::Conflict { key }) if key == "clash"
    ));
    assert_eq!(
        fs::read_to_string(&dst).unwrap(),
        "不是目录\n",
        "拒绝时普通文件原样保留"
    );
    assert!(
        paths.skill_workdir("clash").is_dir(),
        "拒绝时工作副本留在原处"
    );
}

#[test]
fn archive_removes_workdir_and_commits() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add_key(
        &root(&install),
        &make_skill(src_dir.path(), "arch", "归档供版"),
        None,
        false,
    )
    .unwrap();

    let outcome = archive_key(&root(&install), "arch", false).unwrap();
    assert_eq!(outcome.applied, vec!["arch".to_string()]);
    assert_eq!(outcome.code(), 0);
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
    // 归档产生一条机器提交，历史可查
    let repo = git2::Repository::open(paths.root()).unwrap();
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.message().unwrap(), "archive(store): 根 1 个技能");
    assert!(!git::is_dirty(&repo).unwrap());
}

/// 选中集里一个技能都没有：计划里没有移动步也没有提交步，盘上什么都不发生
#[test]
fn archive_of_an_empty_set_does_nothing() {
    let (install, paths) = setup();
    let before = git2::Repository::open(paths.root())
        .unwrap()
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .id();

    let outcome = archive(&root(&install), &Selector::all(Some("k1".into())), false).unwrap();
    assert!(outcome.applied.is_empty() && outcome.skipped.is_empty());
    assert_eq!(outcome.code(), 0, "没有跳过项就不算失败");
    let now = git2::Repository::open(paths.root())
        .unwrap()
        .head()
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .id();
    assert_eq!(before, now, "没有要动的东西就没有提交");
}
