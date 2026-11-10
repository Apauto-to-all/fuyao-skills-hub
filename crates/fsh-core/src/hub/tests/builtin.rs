//! 内置技能对齐的测试：注入自造的内嵌表，钉住落地 / 覆盖 / 发版 / 清理 / 预演五条规则。

use std::fs;
use std::path::Path;

use crate::git;
use crate::hub::sync_builtin;

use super::{builtin_skill, builtin_tags, head_oid, setup_with_empty_builtin};

#[test]
fn builtin_sync_overwrites_changed_content_without_new_tag() {
    let (_install, paths) = setup_with_empty_builtin();
    sync_builtin(&paths, &[builtin_skill("demo", "0.1.0", "第一版")], false).unwrap();
    let work = paths.skill_workdir("builtin/demo");
    // 子目录文件也随整份目录落地，字节与内嵌一致
    assert_eq!(
        fs::read_to_string(work.join("references/a.txt")).unwrap(),
        "ref:第一版"
    );
    assert_eq!(builtin_tags(&paths, "builtin/demo"), vec!["0.1.0"]);

    // 工作副本被改脏、多出根文件与子目录文件：整份覆盖会一起收掉
    fs::write(work.join("SKILL.md"), "被改坏").unwrap();
    fs::write(work.join("references/a.txt"), "被改坏").unwrap();
    fs::write(work.join("extra.md"), "多余").unwrap();
    fs::create_dir_all(work.join("nested")).unwrap();
    fs::write(work.join("nested/extra.md"), "多余").unwrap();
    let head_before = head_oid(&paths);
    let actions = sync_builtin(&paths, &[builtin_skill("demo", "0.1.0", "第二版")], false).unwrap();
    assert!(
        actions.iter().any(|a| a == "write store/builtin/demo"),
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
    assert_eq!(
        builtin_tags(&paths, "builtin/demo"),
        vec!["0.1.0"],
        "号没前进不打新 tag"
    );
}

#[test]
fn builtin_sync_tags_when_version_advances() {
    let (_install, paths) = setup_with_empty_builtin();
    sync_builtin(&paths, &[builtin_skill("demo", "0.1.0", "第一版")], false).unwrap();
    sync_builtin(&paths, &[builtin_skill("demo", "0.2.0", "第二版")], false).unwrap();
    assert_eq!(builtin_tags(&paths, "builtin/demo"), vec!["0.2.0", "0.1.0"]);
    let text = fs::read_to_string(paths.skill_workdir("builtin/demo").join("SKILL.md")).unwrap();
    assert!(text.contains("第二版") && text.contains("0.2.0"), "{text}");
}

#[test]
fn builtin_sync_same_content_is_a_noop() {
    let (_install, paths) = setup_with_empty_builtin();
    let skills = [builtin_skill("demo", "0.1.0", "正文")];
    assert!(!sync_builtin(&paths, &skills, false).unwrap().is_empty());
    let head_before = head_oid(&paths);
    let actions = sync_builtin(&paths, &skills, false).unwrap();
    assert!(actions.is_empty(), "内容相同不应有动作: {actions:?}");
    assert_eq!(head_oid(&paths), head_before, "内容相同不得产生提交");
}

#[test]
fn builtin_sync_removes_orphan_directory_and_tag_chain() {
    let (_install, paths) = setup_with_empty_builtin();
    sync_builtin(
        &paths,
        &[
            builtin_skill("alpha", "0.1.0", "甲"),
            builtin_skill("beta", "0.1.0", "乙"),
        ],
        false,
    )
    .unwrap();
    // beta 再走一次发版，攒出两条 tag，验证整条版本链都被清
    sync_builtin(
        &paths,
        &[
            builtin_skill("alpha", "0.1.0", "甲"),
            builtin_skill("beta", "0.2.0", "乙二"),
        ],
        false,
    )
    .unwrap();
    assert_eq!(builtin_tags(&paths, "builtin/beta"), vec!["0.2.0", "0.1.0"]);

    // 内嵌里去掉 beta：目录连同 tag 链一起清，alpha 不动
    let head_before = head_oid(&paths);
    sync_builtin(&paths, &[builtin_skill("alpha", "0.1.0", "甲")], false).unwrap();
    assert!(
        !paths.skill_workdir("builtin/beta").exists(),
        "内嵌里已无 beta，工作副本应删掉"
    );
    assert!(
        builtin_tags(&paths, "builtin/beta").is_empty(),
        "beta 的 tag 链应一起清"
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
    let actions = sync_builtin(&paths, &[builtin_skill("demo", "0.1.0", "正文")], true).unwrap();
    assert_eq!(
        actions,
        vec![
            "write store/builtin/demo",
            "commit: add builtin/demo v0.1.0",
            "tag builtin/demo/v0.1.0",
        ]
    );
    assert!(!paths.store().join("builtin").exists(), "预演不得落盘");
    assert_eq!(head_oid(&paths), head_before, "预演不得产生提交");
}

#[test]
fn builtin_sync_dry_run_lists_overwrite_without_new_tag() {
    let (_install, paths) = setup_with_empty_builtin();
    sync_builtin(&paths, &[builtin_skill("demo", "0.1.0", "第一版")], false).unwrap();
    let work_md = paths.skill_workdir("builtin/demo").join("SKILL.md");
    fs::write(&work_md, "被改坏").unwrap();
    let head_before = head_oid(&paths);

    let actions = sync_builtin(&paths, &[builtin_skill("demo", "0.1.0", "第二版")], true).unwrap();
    assert_eq!(
        actions,
        vec!["write store/builtin/demo", "commit: sync builtin/demo",],
        "号没前进时预演只列写盘与提交"
    );
    assert_eq!(
        fs::read_to_string(&work_md).unwrap(),
        "被改坏",
        "预演不得覆盖工作副本"
    );
    assert_eq!(head_oid(&paths), head_before, "预演不得产生提交");
}

#[test]
fn builtin_sync_dry_run_lists_tag_when_version_advances() {
    let (_install, paths) = setup_with_empty_builtin();
    sync_builtin(&paths, &[builtin_skill("demo", "0.1.0", "第一版")], false).unwrap();
    let actions = sync_builtin(&paths, &[builtin_skill("demo", "0.2.0", "第二版")], true).unwrap();
    assert_eq!(
        actions,
        vec![
            "write store/builtin/demo",
            "commit: sync builtin/demo",
            "tag builtin/demo/v0.2.0",
        ],
        "号前进时预演多列打 tag 那一步"
    );
    assert_eq!(
        builtin_tags(&paths, "builtin/demo"),
        vec!["0.1.0"],
        "预演不得打新 tag"
    );
}

#[test]
fn builtin_sync_dry_run_lists_orphan_removal_and_keeps_disk() {
    let (_install, paths) = setup_with_empty_builtin();
    sync_builtin(
        &paths,
        &[
            builtin_skill("alpha", "0.1.0", "甲"),
            builtin_skill("beta", "0.1.0", "乙"),
        ],
        false,
    )
    .unwrap();
    sync_builtin(
        &paths,
        &[
            builtin_skill("alpha", "0.1.0", "甲"),
            builtin_skill("beta", "0.2.0", "乙二"),
        ],
        false,
    )
    .unwrap();
    let head_before = head_oid(&paths);

    let mut actions = sync_builtin(&paths, &[builtin_skill("alpha", "0.1.0", "甲")], true).unwrap();
    actions.sort();
    let mut expected = vec![
        "remove store/builtin/beta",
        "commit: remove builtin/beta",
        "delete tag builtin/beta/v0.1.0",
        "delete tag builtin/beta/v0.2.0",
    ];
    expected.sort();
    assert_eq!(actions, expected, "孤儿预演应列出删目录、提交与整条 tag 链");
    assert!(
        paths.skill_workdir("builtin/beta").exists(),
        "预演不得删工作副本"
    );
    assert_eq!(
        builtin_tags(&paths, "builtin/beta"),
        vec!["0.2.0", "0.1.0"],
        "预演不得删 tag"
    );
    assert_eq!(head_oid(&paths), head_before, "预演不得产生提交");
}
