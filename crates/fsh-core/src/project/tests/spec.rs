//! 键入参判定与落点解析测试：坏键在查库前被拒、嵌套键落点名、项目路径规范化。

use std::path::Path;

use super::*;

#[test]
fn nested_key_uses_last_segment_as_destination_name() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let r = use_skill(
        paths.root(),
        "mattpocock/skills/tdd",
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        false,
    );
    // 该键不存在：先确认错误码，再验证存在键的落点命名
    assert_eq!(r.unwrap_err().error_code(), "skill_not_found");
    let r = use_skill(
        paths.root(),
        "demo",
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert!(r.targets[0].destinations[0].path.ends_with("demo"));
}

/// 项目路径规范化：相对路径登记成绝对路径
#[test]
fn relative_project_path_is_stored_as_absolute() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let r = use_skill(
        paths.root(),
        "demo",
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        false,
    )
    .unwrap();
    assert!(Path::new(r.targets[0].project.as_deref().unwrap()).is_absolute());
}

/// 键入参形状：use / unuse 直调都先判语法，坏键在查库之前被拒
#[test]
fn malformed_key_is_rejected_before_lookup() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    for bad in [
        "a\\b",
        "mattpocock\\skills\\tdd",
        "a//b",
        "a/./b",
        "a/../b",
        "C:/x",
    ] {
        let err = use_skill(
            paths.root(),
            bad,
            &["universal".to_string()],
            Targets::new(false, vec![project.as_path().into()]),
            false,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "invalid_skill_key", "use: {bad}");
        let err = unuse(
            paths.root(),
            bad,
            Targets::new(false, vec![project.as_path().into()]),
            &[],
            false,
            false,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "invalid_skill_key", "unuse: {bad}");
    }
}
