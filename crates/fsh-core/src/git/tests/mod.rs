//! git 模块测试：初始化、提交、脏检查、自动提交与开仓姿势。

use super::*;
use std::fs;

/// 测试身份（区分机器提交者与人工提交的对照方）
const NAME: &str = "fuyao";
const EMAIL: &str = "fuyao@local";

/// 建一次性仓库：init 后写入 hub.yaml 骨架并首次提交
fn setup() -> (tempfile::TempDir, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let repo = init(dir.path()).unwrap();
    fs::write(dir.path().join("hub.yaml"), "schema_version: 1\n").unwrap();
    commit_all(&repo, "init hub", NAME, EMAIL).unwrap();
    (dir, repo)
}

#[test]
fn init_creates_main_branch_and_disables_crlf() {
    let dir = tempfile::tempdir().unwrap();
    let repo = init(dir.path()).unwrap();
    // 初始分支必须是 main
    let head = repo.find_reference("HEAD").unwrap();
    assert_eq!(
        head.symbolic_target().unwrap(),
        Some("refs/heads/main"),
        "初始分支应为 main"
    );
    // 仓库本地必须关闭换行改写，否则内容哈希会被污染
    let cfg = repo.config().unwrap();
    let autocrlf = cfg.get_string("core.autocrlf").unwrap();
    assert_eq!(autocrlf.to_lowercase(), "false");
}

#[test]
fn init_rejects_existing_repo() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path()).unwrap();
    assert!(matches!(init(dir.path()), Err(GitError::AlreadyRepo(_))));
}

#[test]
fn commit_all_includes_untracked_files() {
    let (dir, repo) = setup();
    // 新增未跟踪文件 + 修改已有文件，一次提交全进去
    fs::write(dir.path().join("new.txt"), "new").unwrap();
    fs::write(dir.path().join("hub.yaml"), "schema_version: 2\n").unwrap();
    let oid = commit_all(&repo, "second", NAME, EMAIL).unwrap();
    let commit = repo.find_commit(oid).unwrap();
    let tree = commit.tree().unwrap();
    assert!(tree.get_name("new.txt").is_some(), "未跟踪文件应被提交");
    let blob = tree
        .get_name("hub.yaml")
        .unwrap()
        .to_object(&repo)
        .unwrap()
        .peel_to_blob()
        .unwrap();
    assert_eq!(blob.content(), b"schema_version: 2\n");
    assert!(!is_dirty(&repo).unwrap(), "提交后工作区应干净");
}

#[test]
fn is_dirty_detects_new_and_committed_states() {
    let (dir, repo) = setup();
    assert!(!is_dirty(&repo).unwrap(), "刚提交完应干净");
    fs::write(dir.path().join("untracked.txt"), "x").unwrap();
    assert!(is_dirty(&repo).unwrap(), "未跟踪文件应算脏");
}

/// hub 写姿势：装配不完整报 hub_not_initialized；装配齐全时拿到凭证即持锁
/// （同进程第二个写入口报 lock_held），并随带 hub 路径族
#[test]
fn write_pose_takes_lock_and_rejects_unassembled_hub() {
    // 装配不完整（空目录）：判定先于任何盘上动作，稳定码由 open_hub 给出
    let bare = tempfile::tempdir().unwrap();
    let err = match open_hub_for_write(bare.path()) {
        Err(e) => e,
        Ok(_) => panic!("装配不完整的目录不得开仓"),
    };
    assert_eq!(crate::ErrorCode::error_code(&err), "hub_not_initialized");

    // 装配齐全：凭证活着期间锁是独占的，释放后又能再取
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let (token, _repo, paths) = open_hub_for_write(install.path()).unwrap();
    assert!(paths.is_initialized(), "写姿势随带可用的路径族");
    assert!(matches!(
        crate::lock::write_lock(&paths),
        Err(crate::lock::LockError::Held)
    ));
    drop(token);
    assert!(open_hub_for_write(install.path()).is_ok());
}

/// 写姿势对没建过的 hub 根零副作用：报 hub_not_initialized，根目录与锁文件
/// 都不许被创建（取锁会建锁文件的父目录，所以装配判定必须先于取锁）
#[test]
fn write_pose_leaves_no_trace_on_unbuilt_hub() {
    let parent = tempfile::tempdir().unwrap();
    let missing = parent.path().join("nope");
    assert!(matches!(
        open_hub_for_write(&missing),
        Err(crate::error::CommonError::HubNotInitialized(_))
    ));
    assert!(!missing.exists(), "写姿势不得凭空创建 hub 根");

    // 根目录存在但没装配过：同样只报错，不留下 .lock
    let bare = tempfile::tempdir().unwrap();
    assert!(open_hub_for_write(bare.path()).is_err());
    assert!(
        !bare.path().join(".lock").exists(),
        "未装配的目录不得留下锁文件"
    );
}

mod auto;
