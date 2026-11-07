//! 预演与写锁的互斥语义：项目面的预演同样只读，别人持锁时也能给出动作清单。

use super::*;

/// 持锁预演 use：写锁被占时预演照样返回动作清单，登记簿与落点零变化
#[test]
fn dry_use_works_while_write_lock_held() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();

    // 模拟并发写：先取全局写锁且不放手
    let _held = crate::lock::write_lock(&paths).unwrap();

    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &two_agents(),
        Targets::project(&project),
        true,
    )
    .unwrap();
    assert!(
        r.targets[0]
            .actions
            .iter()
            .any(|a| a.contains("link") && a.contains("demo")),
        "动作清单应包含建链接: {:?}",
        r.targets[0].actions
    );
    // 预演零落盘：登记簿无记录
    assert!(record(&paths, &project, "demo").is_none());
}

/// 持锁预演 unuse：撤回预演同样不取锁
#[test]
fn dry_unuse_works_while_write_lock_held() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    use_skill(
        paths.root(),
        "demo",
        None,
        &two_agents(),
        Targets::project(&project),
        false,
    )
    .unwrap();

    let _held = crate::lock::write_lock(&paths).unwrap();

    let r = unuse(
        paths.root(),
        "demo",
        Targets::project(&project),
        false,
        true,
    )
    .unwrap();
    let actions = &r.targets[0].actions;
    assert!(
        actions.iter().any(|a| a.starts_with("remove ")),
        "动作清单应包含删落点: {actions:?}"
    );
    // 预演零落盘：登记记录仍在
    assert!(record(&paths, &project, "demo").is_some());
}
