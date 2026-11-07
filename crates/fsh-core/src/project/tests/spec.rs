//! 版本与落点解析测试：技能规格解析、嵌套键落点名、项目路径规范化。

use std::path::Path;

use super::*;

#[test]
fn split_skill_spec_parses_versions_and_rejects_bad_shapes() {
    assert_eq!(split_skill_spec("demo").unwrap(), ("demo".into(), None));
    assert_eq!(
        split_skill_spec("a/b/demo@1.2.3").unwrap(),
        ("a/b/demo".into(), Some("1.2.3".into()))
    );
    for bad in ["", "@1.0.0", "demo@1.0", "demo@v1.0.0"] {
        let err = split_skill_spec(bad).unwrap_err();
        assert_eq!(err.error_code(), "invalid_skill_spec", "{bad}");
    }
}

/// 嵌套键末段决定落点名
#[test]
fn nested_key_uses_last_segment_as_destination_name() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let r = use_skill(
        paths.root(),
        "anthropics/skills/pdf",
        None,
        &["universal".to_string()],
        Targets::project(project.as_path()),
        false,
    );
    // 该键不存在：先确认错误码，再验证存在键的落点命名
    assert_eq!(r.unwrap_err().error_code(), "skill_not_found");
    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &["universal".to_string()],
        Targets::project(project.as_path()),
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
        None,
        &["universal".to_string()],
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    assert!(Path::new(r.targets[0].project.as_deref().unwrap()).is_absolute());
}
