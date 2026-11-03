//! 升级与预演测试：撤回后按固定版本重新使用，预演只看报告不落盘。

use super::*;

#[test]
fn use_after_unuse_upgrades_to_fixed_version() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        project.as_path(),
        false,
    )
    .unwrap();
    unuse(paths.root(), "demo", project.as_path(), false, false).unwrap();
    let r = use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        project.as_path(),
        false,
    )
    .unwrap();
    assert_eq!(r.version.as_deref(), Some("1.0.0"));
    assert_eq!(
        record(&paths, project.as_path(), "demo")
            .unwrap()
            .version
            .as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn use_dry_run_touches_nothing() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = two_agents();
    let actions = use_dry(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        project.as_path(),
    )
    .unwrap();
    assert!(actions.iter().any(|a| a.contains("export")));
    assert!(
        actions
            .iter()
            .any(|a| a.contains(".dsh") && a.contains("demo")),
        "预演应列出 dsh 落点: {actions:?}"
    );
    assert!(!paths.exported("demo", "1.0.0").exists(), "预演不得导出");
    assert!(
        !project.as_path().join(".agents/skills/demo").exists(),
        "预演不得建链接"
    );
    assert!(
        record(&paths, project.as_path(), "demo").is_none(),
        "预演不写登记簿"
    );
}
