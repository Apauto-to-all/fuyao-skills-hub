//! 内置技能对齐的测试：注入自造的内嵌表，钉住落地 / 覆盖 / 清理 / 预演四条规则。

use std::fs;
use std::path::Path;

use crate::git;

use crate::plan::{ActionKind, PlanStep};

use super::{builtin_skill, head_oid, land_skills, setup_with_empty_builtin};

#[test]
fn builtin_sync_overwrites_changed_content() {
    let (_install, paths) = setup_with_empty_builtin();
    land_skills(&paths, &[builtin_skill("demo", "第一版")], false).unwrap();
    let work = paths.skill_workdir("builtin/demo");
    // 子目录文件也随整份目录落地，字节与内嵌一致
    assert_eq!(
        fs::read_to_string(work.join("references/a.txt")).unwrap(),
        "ref:第一版"
    );

    // 工作副本被改脏、多出根文件与子目录文件：整份覆盖会一起收掉
    fs::write(work.join("SKILL.md"), "被改坏").unwrap();
    fs::write(work.join("references/a.txt"), "被改坏").unwrap();
    fs::write(work.join("extra.md"), "多余").unwrap();
    fs::create_dir_all(work.join("nested")).unwrap();
    fs::write(work.join("nested/extra.md"), "多余").unwrap();
    let head_before = head_oid(&paths);
    let actions = land_skills(&paths, &[builtin_skill("demo", "第二版")], false).unwrap();
    assert!(
        actions.contains(&PlanStep::of(ActionKind::Write, "store/builtin/demo")),
        "动作清单应写出写盘那一步: {actions:?}"
    );
    let text = fs::read_to_string(work.join("SKILL.md")).unwrap();
    assert!(text.contains("第二版"), "应整份覆盖为内嵌内容: {text}");
    assert_eq!(
        fs::read_to_string(work.join("references/a.txt")).unwrap(),
        "ref:第二版",
        "子目录文件也要整份覆盖"
    );
    assert!(
        !work.join("extra.md").exists() && !work.join("nested").exists(),
        "整份覆盖应删掉多余文件与目录"
    );
    assert_ne!(head_oid(&paths), head_before, "整份覆盖要提交");
}

#[test]
fn builtin_sync_same_content_is_a_noop() {
    let (_install, paths) = setup_with_empty_builtin();
    let skills = [builtin_skill("demo", "正文")];
    assert!(!land_skills(&paths, &skills, false).unwrap().is_empty());
    let head_before = head_oid(&paths);
    let actions = land_skills(&paths, &skills, false).unwrap();
    assert!(actions.is_empty(), "内容相同不应有动作: {actions:?}");
    assert_eq!(head_oid(&paths), head_before, "内容相同不得产生提交");
}

/// 多个内置技能一起变更也只提交一次：预演列的提交步必须与实跑的次数一致
#[test]
fn builtin_sync_commits_all_changes_once() {
    let (_install, paths) = setup_with_empty_builtin();
    let skills = [builtin_skill("alpha", "甲"), builtin_skill("beta", "乙")];
    let actions = land_skills(&paths, &skills, true).unwrap();
    let commits: Vec<&PlanStep> = actions
        .iter()
        .filter(|step| step.action == ActionKind::Commit)
        .collect();
    assert_eq!(
        commits.len(),
        1,
        "多技能一次对齐只列一条提交步: {actions:?}"
    );
    assert_eq!(commits[0].target.as_deref(), Some("init(builtin): builtin"));

    // 实跑：HEAD 只前进一个提交，标题就是那条提交步
    let head_before = head_oid(&paths);
    land_skills(&paths, &skills, false).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(head.message().unwrap(), "init(builtin): builtin");
    assert_eq!(
        Some(head.parent(0).unwrap().id()),
        head_before,
        "一次对齐只前进一个提交"
    );
}

#[test]
fn builtin_sync_removes_orphan_directory() {
    let (_install, paths) = setup_with_empty_builtin();
    land_skills(
        &paths,
        &[builtin_skill("alpha", "甲"), builtin_skill("beta", "乙")],
        false,
    )
    .unwrap();

    // 内嵌里去掉 beta：目录整份删掉，alpha 不动
    let head_before = head_oid(&paths);
    land_skills(&paths, &[builtin_skill("alpha", "甲")], false).unwrap();
    assert!(
        !paths.skill_workdir("builtin/beta").exists(),
        "内嵌里已无 beta，工作副本应删掉"
    );
    assert_ne!(head_oid(&paths), head_before, "删除要提交");
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(!git::is_dirty(&repo).unwrap(), "删除后工作区应干净");
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    assert!(
        tree.get_path(Path::new("store/builtin/beta")).is_err(),
        "HEAD 树里不该再有该子树"
    );
    assert!(paths.skill_workdir("builtin/alpha/SKILL.md").is_file());
}

#[test]
fn builtin_sync_dry_run_lists_actions_and_writes_nothing() {
    let (_install, paths) = setup_with_empty_builtin();
    let head_before = head_oid(&paths);
    let actions = land_skills(&paths, &[builtin_skill("demo", "正文")], true).unwrap();
    assert_eq!(
        actions,
        vec![
            PlanStep::of(ActionKind::Write, "store/builtin/demo"),
            PlanStep::commit("init(builtin): builtin"),
        ]
    );
    assert!(!paths.store().join("builtin").exists(), "预演不得落盘");
    assert_eq!(head_oid(&paths), head_before, "预演不得产生提交");
}

#[test]
fn builtin_sync_dry_run_lists_overwrite_without_touching_disk() {
    let (_install, paths) = setup_with_empty_builtin();
    land_skills(&paths, &[builtin_skill("demo", "第一版")], false).unwrap();
    let work_md = paths.skill_workdir("builtin/demo").join("SKILL.md");
    fs::write(&work_md, "被改坏").unwrap();
    let head_before = head_oid(&paths);

    let actions = land_skills(&paths, &[builtin_skill("demo", "第二版")], true).unwrap();
    assert_eq!(
        actions,
        vec![
            PlanStep::of(ActionKind::Write, "store/builtin/demo"),
            PlanStep::commit("init(builtin): builtin"),
        ]
    );
    assert_eq!(
        fs::read_to_string(&work_md).unwrap(),
        "被改坏",
        "预演不得覆盖工作副本"
    );
    assert_eq!(head_oid(&paths), head_before, "预演不得产生提交");
}

#[test]
fn builtin_sync_dry_run_lists_orphan_removal_and_keeps_disk() {
    let (_install, paths) = setup_with_empty_builtin();
    land_skills(
        &paths,
        &[builtin_skill("alpha", "甲"), builtin_skill("beta", "乙")],
        false,
    )
    .unwrap();
    let head_before = head_oid(&paths);

    let actions = land_skills(&paths, &[builtin_skill("alpha", "甲")], true).unwrap();
    assert_eq!(
        actions,
        vec![
            PlanStep::of(ActionKind::Remove, "store/builtin/beta"),
            PlanStep::commit("init(builtin): builtin"),
        ],
        "孤儿预演应列出删目录与提交"
    );
    assert!(
        paths.skill_workdir("builtin/beta").exists(),
        "预演不得删工作副本"
    );
    assert_eq!(head_oid(&paths), head_before, "预演不得产生提交");
}
