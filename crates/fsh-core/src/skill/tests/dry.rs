//! 预演与写锁的互斥语义：预演只读不取锁，别人正在写时也能给出动作清单。

use super::*;

/// 持锁预演：另一进程正在写（全局写锁被占）时，预演照样返回完整动作清单，
/// 而不是把「锁被占」误报给只想预估的用户
#[test]
fn dry_add_works_while_write_lock_held() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例技能");

    // 模拟并发写：先取全局写锁且不放手
    let _held = crate::lock::write_lock(&paths).unwrap();

    let r = add(&root(&install), &src, None, true).unwrap();
    assert!(r.actions.iter().any(|a| a.contains("tag demo/v0.1.0")));
    // 预演零落盘
    assert!(!paths.skill_workdir("demo").exists());
}

/// 对照组：实跑在锁被占时必须报 lock_held——预演免锁不是写路径免锁
#[test]
fn real_add_reports_lock_held_when_occupied() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例技能");
    let _held = crate::lock::write_lock(&paths).unwrap();

    let err = add(&root(&install), &src, None, false).unwrap_err();
    assert_eq!(err.error_code(), "lock_held");
}
