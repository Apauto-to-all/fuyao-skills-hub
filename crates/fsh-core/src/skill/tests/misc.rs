//! 杂项测试：预演零落盘、同名共存、克隆携带全部技能信息。

use std::fs;

use super::*;

#[test]
fn dry_runs_plan_actions_without_touching_disk() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "peek", None, "预演");
    // add 预演：动作清单齐全，store 一字未动
    let actions = add(&root(&install), &src, None, true).unwrap().actions;
    assert!(actions.iter().any(|a| a.contains("tag peek/v0.1.0")));
    // 缺省号写回也要出现在预演里，与真 add 的动作面对齐
    assert!(actions.iter().any(|a| a == "写回 metadata.version = 0.1.0"));
    assert!(paths.store().read_dir().unwrap().next().is_none());

    add(&root(&install), &src, None, false).unwrap();
    // release 预演：点名目标号，不产生新 tag
    set_version(&paths, "peek", "0.2.0");
    let actions = release(
        &root(&install),
        "peek",
        ReleaseSource::Frontmatter,
        false,
        true,
    )
    .unwrap()
    .actions;
    assert!(actions.iter().any(|a| a == "commit: release peek v0.2.0"));
    assert!(actions.iter().any(|a| a == "tag peek/v0.2.0"));
    assert_eq!(
        git::tag_list(&git2::Repository::open(paths.root()).unwrap(), "peek").unwrap(),
        vec!["0.1.0"]
    );

    // archive 预演：列出将删的编辑面与导出目录，盘上原封不动
    let actions = archive(&root(&install), "peek", true).unwrap().actions;
    assert!(actions.iter().any(|a| a == "remove store/peek"));
    assert!(actions.iter().any(|a| a == "remove export/peek/*"));
    assert!(paths.skill_workdir("peek").is_dir());

    // remove 预演：列出将删的 tag，盘上工作副本与 tag 都还在
    let actions = remove(&root(&install), "peek", true).unwrap().actions;
    assert!(actions.iter().any(|a| a == "remove store/peek"));
    assert!(actions.iter().any(|a| a == "delete tag peek/v0.1.0"));
    assert!(paths.skill_workdir("peek").is_dir());

    archive(&root(&install), "peek", false).unwrap();
    // restore 预演：点名最新 tag，工作副本仍未复活
    let actions = restore(&root(&install), "peek", true).unwrap().actions;
    assert!(
        actions
            .iter()
            .any(|a| a == "checkout store/peek @ peek/v0.1.0")
    );
    assert!(!paths.skill_workdir("peek").is_dir());
}

#[test]
fn same_name_skills_coexist_under_different_keys() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let nested = make_skill(src_dir.path(), "tdd", None, "外来的");
    add(&root(&install), &nested, Some("mattpocock/skills"), false).unwrap();
    let own = make_skill(src_dir.path(), "tdd", None, "自产的");
    add(&root(&install), &own, None, false).unwrap();

    // 各自独立：两个键都在库中
    assert!(paths.skill_workdir("mattpocock/skills/tdd").is_dir());
    assert!(paths.skill_workdir("tdd").is_dir());

    // 各自可发版、互不干扰
    set_version(&paths, "tdd", "0.1.1");
    release(
        &root(&install),
        "tdd",
        ReleaseSource::Frontmatter,
        false,
        false,
    )
    .unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "tdd").unwrap(), vec!["0.1.1", "0.1.0"]);
    assert_eq!(
        git::tag_list(&repo, "mattpocock/skills/tdd").unwrap(),
        vec!["0.1.0"]
    );

    // 归档一个，另一个原封不动
    archive(&root(&install), "tdd", false).unwrap();
    assert!(paths.skill_workdir("mattpocock/skills/tdd").is_dir());
    assert_eq!(
        git::tag_list(&repo, "mattpocock/skills/tdd").unwrap(),
        vec!["0.1.0"]
    );
}

#[test]
fn clone_carries_all_skill_info_without_external_ledger() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "clone", None, "跟库走");
    fs::write(
        src.join("SKILL.md"),
        "---\nname: clone\ndescription: 跟库走\nlicense: MIT\nmetadata:\n  version: \"0.3.0\"\n  source: owner/repo\n  tags: \"a, b\"\n---\n\n# clone\n正文\n",
    )
    .unwrap();
    add(&root(&install), &src, None, false).unwrap();

    // 克隆 hub 仓库：内容 + 元数据 + 版本历史全在 git 里
    let dest = tempfile::tempdir().unwrap();
    let clone_root = dest.path().to_path_buf();
    git2::Repository::clone(paths.root().to_str().unwrap(), &clone_root).unwrap();

    // 查询结果与原库逐字段一致（没有任何库外账本参与）
    assert_eq!(
        show(&clone_root, "clone").unwrap(),
        show(paths.root(), "clone").unwrap()
    );
    assert!(!clone_root.join("meta").exists());
}
