//! 自动提交测试：脏检查跳过、机器提交者身份，以及写命令全链路的提交形态
//! （消息遵循 `<动词>(<目标>): <对象>`，禁止绝对路径与盘符）。

use super::*;
use std::fs;
use std::path::Path;

/// 提交消息隐私红线：不含绝对路径、反斜杠与盘符形态（`X:`）
fn assert_relative_message(message: &str, hub_root: &Path) {
    assert!(
        !message.contains(&hub_root.display().to_string()),
        "提交消息不得含 hub 绝对路径: {message}"
    );
    assert!(!message.contains('\\'), "提交消息不得含反斜杠: {message}");
    let has_drive = message
        .char_indices()
        .any(|(i, c)| c.is_ascii_alphabetic() && message[i + 1..].starts_with(':'));
    assert!(!has_drive, "提交消息不得含盘符形态: {message}");
}

/// HEAD 提交的 (消息, 提交者名, 提交者邮箱)
fn head_commit(repo: &Repository) -> (String, String, String) {
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    (
        commit.message().unwrap_or_default().to_string(),
        commit.committer().name().unwrap_or_default().to_string(),
        commit.committer().email().unwrap_or_default().to_string(),
    )
}

/// HEAD 提交 id
fn head_id(repo: &Repository) -> git2::Oid {
    repo.head().unwrap().peel_to_commit().unwrap().id()
}

/// 工作区干净时静默跳过：HEAD 不动、不产生空提交
#[test]
fn auto_commit_skips_clean_workdir() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init(dir.path()).unwrap();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    auto_commit(&repo, "add(store): demo");
    let after_first = head_id(&repo);
    // 第二次没有任何变更：HEAD 原样不动
    auto_commit(&repo, "add(store): demo");
    assert_eq!(head_id(&repo), after_first, "无变更不得产生空提交");
    assert!(!is_dirty(&repo).unwrap());
}

/// 有变更时产生机器提交：固定提交者身份与消息原样落进 HEAD
#[test]
fn auto_commit_uses_fixed_machine_committer() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init(dir.path()).unwrap();
    fs::write(dir.path().join("a.txt"), "a").unwrap();
    auto_commit(&repo, "add(store): demo");
    let (message, name, email) = head_commit(&repo);
    assert_eq!(message, "add(store): demo");
    assert_eq!((name.as_str(), email.as_str()), ("fsh", "fsh@local"));
}

/// 造一个源 skill 目录（带 frontmatter）
fn make_skill(dir: &Path, name: &str) -> std::path::PathBuf {
    let skill_dir = dir.join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 演示\n---\n\n正文\n"),
    )
    .unwrap();
    skill_dir
}

/// 写命令各产生一条机器提交：init / add / use / unuse / archive / remove。
/// use 与 unuse 只在 hub 工作区有变更时才有提交可做（落点在项目侧、登记簿
/// ignored），这里先把工作副本弄脏再执行；每条消息只含键名、agent 名与
/// 项目名，隐私红线（无绝对路径与盘符）逐条断言
#[test]
fn write_commands_each_produce_one_machine_commit() {
    let install = tempfile::tempdir().unwrap();
    let root = install.path();
    crate::hub::init(root, false).unwrap();
    let repo = crate::git::open_hub(root).unwrap();
    let (init_msg, init_name, _) = head_commit(&repo);
    assert_eq!(init_name, "fsh", "init 落盘后应是机器提交");
    assert_relative_message(&init_msg, root);

    // add：入库产生一条提交
    let src_root = tempfile::tempdir().unwrap();
    let src = make_skill(src_root.path(), "demo");
    crate::skill::add(root, &src, None, false).unwrap();
    let (add_msg, _, _) = head_commit(&repo);
    assert_eq!(add_msg, "add(store): demo");
    assert_relative_message(&add_msg, root);

    // use：铺给项目产生一条提交（工作副本先弄脏，提交才有内容）
    let proj = tempfile::tempdir().unwrap();
    let project_dir = proj.keep();
    fs::write(
        paths(root).skill_workdir("demo").join("SKILL.md"),
        "# dirty\n",
    )
    .unwrap();
    let targets = crate::project::Targets::new(false, vec![project_dir.clone()]);
    crate::project::use_skill(root, "demo", &["universal".to_string()], targets, false).unwrap();
    let (use_msg, _, _) = head_commit(&repo);
    assert_eq!(
        use_msg,
        format!(
            "use(universal): demo -> {}",
            project_dir.file_name().unwrap().to_string_lossy()
        )
    );
    assert_relative_message(&use_msg, root);

    // unuse：撤回产生一条提交（同上先弄脏）
    fs::write(
        paths(root).skill_workdir("demo").join("SKILL.md"),
        "# dirty2\n",
    )
    .unwrap();
    let targets = crate::project::Targets::new(false, vec![project_dir.clone()]);
    crate::project::unuse(root, "demo", targets, None, false, false).unwrap();
    let (unuse_msg, _, _) = head_commit(&repo);
    assert_eq!(
        unuse_msg,
        format!(
            "unuse(universal): demo -> {}",
            project_dir.file_name().unwrap().to_string_lossy()
        )
    );
    assert_relative_message(&unuse_msg, root);

    // archive：归档产生一条提交
    crate::skill::archive(root, "demo", false).unwrap();
    let (archive_msg, _, _) = head_commit(&repo);
    assert_eq!(archive_msg, "archive(store): demo");
    assert_relative_message(&archive_msg, root);

    // remove：彻底删除产生一条提交（归档后键已不在，另入库一个技能来删）
    let src2 = make_skill(src_root.path(), "demo2");
    crate::skill::add(root, &src2, None, false).unwrap();
    crate::skill::remove(root, "demo2", false).unwrap();
    let (remove_msg, _, _) = head_commit(&repo);
    assert_eq!(remove_msg, "remove(store): demo2");
    assert_relative_message(&remove_msg, root);
}

/// hub 路径族（测试观察用）
fn paths(root: &Path) -> crate::paths::HubPaths {
    crate::paths::HubPaths::from_root(root)
}

/// 预演不落盘即不提交：dry-run 前后 HEAD 不动
#[test]
fn dry_run_produces_no_commit() {
    let install = tempfile::tempdir().unwrap();
    let root = install.path();
    crate::hub::init(root, false).unwrap();
    let repo = crate::git::open_hub(root).unwrap();
    let before = head_id(&repo);

    let src_root = tempfile::tempdir().unwrap();
    let src = make_skill(src_root.path(), "demo2");
    crate::skill::add(root, &src, None, true).unwrap();
    assert_eq!(head_id(&repo), before, "dry-run 不得产生提交");

    crate::skill::add(root, &src, None, false).unwrap();
    let after_add = head_id(&repo);
    assert_ne!(after_add, before);
    crate::skill::archive(root, "demo2", true).unwrap();
    assert_eq!(head_id(&repo), after_add, "dry-run 不得产生提交");
}
