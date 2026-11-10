//! 撤回测试：全部落点删除与记录清除、幂等、手改副本保护、预演零落盘、多目标组合。

use std::fs;

use super::*;
use crate::hub;

#[test]
fn unuse_removes_all_destinations_and_record() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = two_agents();
    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();

    let r = unuse(
        paths.root(),
        "demo",
        Targets::new(false, vec![project.as_path().into()]),
        None,
        false,
        false,
    )
    .unwrap();
    assert_eq!(r.targets[0].removed.len(), 2);
    assert!(!project.as_path().join(".agents/skills/demo").exists());
    assert!(!project.as_path().join(".dsh/skills/demo").exists());
    assert!(record(&paths, project.as_path(), "demo").is_none());
    // 项目没有其他技能，项目记录也清掉
    let reg = hub::load_registry(&paths).unwrap();
    assert!(reg.projects.is_empty());
}

#[test]
fn unuse_is_idempotent_on_missing_destination_but_not_missing_record() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = two_agents();
    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    fs::remove_dir_all(project.as_path().join(".dsh/skills/demo")).unwrap();

    // 缺失落点视为已撤回：报告只列真删掉的那一个
    let r = unuse(
        paths.root(),
        "demo",
        Targets::new(false, vec![project.as_path().into()]),
        None,
        false,
        false,
    )
    .unwrap();
    assert_eq!(
        r.targets[0].removed.len(),
        1,
        "缺失落点不该出现在报告里: {:?}",
        r.targets[0].removed
    );
    // 记录没了：再次撤回报错，不扫描黑链接
    let err = unuse(
        paths.root(),
        "demo",
        Targets::new(false, vec![project.as_path().into()]),
        None,
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "not_registered");
}

#[test]
fn unuse_protects_modified_copy_unless_forced() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = ["universal".to_string()];
    // 真实 use 的档位取决于本机三级降级结果：copy 档断言修改保护，
    // 链接档断言被替换后始终拒绝（force 也不覆盖）
    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    let dest = project.as_path().join(".agents/skills/demo");
    if r.targets[0].link == Some(LinkKind::Copy) {
        fs::write(dest.join("SKILL.md"), "手改内容").unwrap();
        let err = unuse(
            paths.root(),
            "demo",
            Targets::new(false, vec![project.as_path().into()]),
            None,
            false,
            true,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "modified_copy");
        assert!(dest.is_dir(), "拒绝时不得删除");
        unuse(
            paths.root(),
            "demo",
            Targets::new(false, vec![project.as_path().into()]),
            None,
            true,
            false,
        )
        .unwrap();
        assert!(!dest.exists());
    } else {
        // 链接档位：落点被换成普通目录时必须拒绝，force 也不覆盖
        fs::remove_dir(&dest).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("user.txt"), "keep").unwrap();
        for force in [false, true] {
            let err = unuse(
                paths.root(),
                "demo",
                Targets::new(false, vec![project.as_path().into()]),
                None,
                force,
                false,
            )
            .unwrap_err();
            assert_eq!(err.error_code(), "link_kind_mismatch");
            assert!(dest.join("user.txt").is_file(), "force 也不得覆盖");
        }
    }
}

#[test]
fn unuse_dry_run_touches_nothing() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = two_agents();
    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    let actions = unuse(
        paths.root(),
        "demo",
        Targets::new(false, vec![project.as_path().into()]),
        None,
        false,
        true,
    )
    .unwrap()
    .targets[0]
        .actions
        .clone();
    assert_eq!(actions.len(), 2);
    assert!(
        project.as_path().join(".agents/skills/demo").exists(),
        "预演不得删"
    );
    assert!(
        record(&paths, project.as_path(), "demo").is_some(),
        "预演不写登记簿"
    );
}
