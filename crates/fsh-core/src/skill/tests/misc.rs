//! 杂项测试：预演零落盘、同名共存、克隆携带全部技能信息。

use std::fs;

use super::*;

#[test]
fn dry_runs_plan_actions_without_touching_disk() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "peek", None, "预演");
    // add 预演：动作清单齐全，store 一字未动（peek 的工作副本不落地）
    let actions = add(&root(&install), &src, None, true).unwrap().actions;
    assert!(actions.iter().any(|a| a == "commit: add(store): peek"));
    assert!(!paths.skill_workdir("peek").exists());
    // 预演不落盘即不提交
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head_before = repo.head().unwrap().peel_to_commit().unwrap().id();

    add(&root(&install), &src, None, false).unwrap();

    // archive 预演：列出将移动的两个面，盘上原封不动
    let actions = archive(&root(&install), "peek", true).unwrap().actions;
    assert!(
        actions
            .iter()
            .any(|a| a == "move store/peek -> archive/peek")
    );
    assert!(paths.skill_workdir("peek").is_dir());
    assert!(!paths.skill_archived("peek").exists());

    // remove 预演：列出将删的工作副本，盘上原封不动
    let actions = remove(&root(&install), "peek", true).unwrap().actions;
    assert!(actions.iter().any(|a| a == "remove store/peek"));
    assert!(paths.skill_workdir("peek").is_dir());

    remove(&root(&install), "peek", false).unwrap();
    assert!(!paths.skill_workdir("peek").exists());
    assert!(!paths.skill_archived("peek").exists());
    assert!(repo.head().unwrap().peel_to_commit().unwrap().id() != head_before);
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

    // 归档一个：整目录移入归档面，另一个原封不动
    archive(&root(&install), "tdd", false).unwrap();
    assert!(!paths.skill_workdir("tdd").exists());
    assert!(paths.skill_archived("tdd").join("SKILL.md").is_file());
    assert!(paths.skill_workdir("mattpocock/skills/tdd").is_dir());
}

#[test]
fn clone_carries_all_skill_info_without_external_ledger() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "clone", None, "跟库走");
    fs::write(
        src.join("SKILL.md"),
        "---\nname: clone\ndescription: 跟库走\nlicense: MIT\nmetadata:\n  version: \"0.3.0\"\n  source: owner/repo\n  tags:\n    - a\n    - b\n---\n\n# clone\n正文\n",
    )
    .unwrap();
    add(&root(&install), &src, None, false).unwrap();

    // 克隆 hub 仓库：内容 + 元数据全在 git 里
    let dest = tempfile::tempdir().unwrap();
    let clone_root = dest.path().to_path_buf();
    git2::Repository::clone(paths.root().to_str().unwrap(), &clone_root).unwrap();
    let cloned =
        fs::read_to_string(clone_root.join("store").join("clone").join("SKILL.md")).unwrap();
    assert!(
        cloned.contains("license: MIT") && cloned.contains("source: owner/repo"),
        "frontmatter 全字段随内容树走"
    );
}
