//! 键入参判定与落点解析测试：坏键在查库前被拒、嵌套键落点名、项目路径规范化。

use super::*;

/// 落点名 = 键末段：多级键入库后落点只取最后一段，前缀不进目录名
#[test]
fn nested_key_uses_last_segment_as_destination_name() {
    let (_install, paths, key) = setup_with_nested_skill();
    let project = temp_project();
    let (prefix, name) = key.rsplit_once('/').unwrap();
    let r = use_skill(
        paths.root(),
        &Selector::named(Some(prefix.to_string()), vec![name.to_string()]),
        &["universal".to_string()],
        Targets::new(false, vec![project.as_path().into()]),
        None,
        false,
    )
    .unwrap();
    assert_eq!(r.applied, [key]);
    let dest = project.as_path().join(".agents/skills/tdd");
    assert!(
        dest.ends_with(".agents/skills/tdd"),
        "落点名须是键末段，前缀不参与: {}",
        dest.display()
    );
    assert!(dest.join("SKILL.md").is_file());
}

/// 键入参形状：use / unuse 直调都先判语法，坏键在查库之前被拒
#[test]
fn malformed_key_is_rejected_before_lookup() {
    let (_install, paths) = setup_with_skill();
    let project = temp_project();
    for bad in [
        "a\\b",
        "mattpocock\\skills\\tdd",
        "a//b",
        "a/./b",
        "a/../b",
        "C:/x",
    ] {
        // 前缀与技能名是两个入口，形状判定各在各自那一段上生效
        for selection in [
            Selector::named(Some(bad.to_string()), vec!["demo".to_string()]),
            Selector::named(None, vec![bad.to_string()]),
        ] {
            let err = use_skill(
                paths.root(),
                &selection,
                &["universal".to_string()],
                Targets::new(false, vec![project.as_path().into()]),
                None,
                false,
            )
            .unwrap_err();
            assert_eq!(err.error_code(), "invalid_skill_key", "use: {bad}");
        }
        let err = unuse(
            paths.root(),
            &Selector::named(None, vec![bad.to_string()]),
            Targets::new(false, vec![project.as_path().into()]),
            &[],
            false,
            false,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "invalid_skill_key", "unuse: {bad}");
    }
}
