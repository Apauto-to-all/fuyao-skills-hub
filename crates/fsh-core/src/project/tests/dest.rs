//! 落点与导出守卫测试：父目录占用、导出目录一致性、建链失败回滚。

use std::fs;

use super::*;

/// 落点父目录链上有同名文件时视为占用
#[test]
fn use_rejects_parent_path_occupied_by_file() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    fs::write(project.as_path().join(".agents"), "not a dir").unwrap();
    let err = use_skill(
        paths.root(),
        "demo",
        None,
        &["universal".to_string()],
        project.as_path(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "destination_occupied");
}

/// 导出目录被手改后 use 拒绝，且不改动已有落点
#[test]
fn dirty_export_is_not_overwritten_and_no_link_created() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &["universal".to_string()],
        project.as_path(),
        false,
    )
    .unwrap();
    unuse(paths.root(), "demo", project.as_path(), false, false).unwrap();
    fs::write(paths.exported("demo", "1.0.0").join("x"), "dirty").unwrap();
    let other = temp_project();
    let err = use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &["universal".to_string()],
        other.as_path(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "dirty_export");
    assert!(!other.as_path().join(".agents/skills/demo").exists());
}

#[test]
fn use_reuses_matching_export_and_rejects_dirty_export() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    // 先正常导出一次，再撤回，取得一份「已存在」的导出目录用于复用校验
    use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        project.as_path(),
        false,
    )
    .unwrap();
    unuse(paths.root(), "demo", project.as_path(), false, false).unwrap();
    let export = paths.exported("demo", "1.0.0");
    // 回收策略由 hub 配置决定，这里只要一份现存导出目录
    if !export.is_dir() {
        let repo = git2::Repository::open(paths.root()).unwrap();
        git::materialize(&repo, "demo/v1.0.0", "store/demo", &export).unwrap();
    }
    assert!(export.is_dir());

    // 一致复用：同一版本再 use 成功
    let project2 = temp_project();
    use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        project2.as_path(),
        false,
    )
    .unwrap();

    // 改成脏目录后：报错并保留现场（先撤回 project2，导出目录随之无引用）
    unuse(paths.root(), "demo", project2.as_path(), false, false).unwrap();
    fs::write(export.join("hand-edit.txt"), "x").unwrap();
    let project3 = temp_project();
    let err = use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        project3.as_path(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "dirty_export");
    assert!(export.join("hand-edit.txt").is_file(), "脏目录绝不覆盖");
}

#[test]
fn failed_link_creation_rolls_back_created_destinations() {
    // 第二个落点的父目录被同名文件占住时，建链会在中途失败；
    // 预检已挡住这种输入，这里直接验证回滚原语：已建入口必须被清干净
    let project = temp_project();
    let first = project.join(".agents/skills/demo");
    let second = project.join(".dsh/skills/demo");
    fs::create_dir_all(first.parent().unwrap()).unwrap();
    let kind = link::create_dir_link(&project, &first).unwrap();
    // 第二个落点建不出来：父路径是文件
    fs::create_dir_all(second.parent().unwrap()).unwrap();
    fs::write(second.parent().unwrap().join("demo"), "occupied").unwrap();
    assert!(
        link::create_dir_link(&project, &second).is_err(),
        "占用落点必须失败"
    );
    rollback_links(&[(first.clone(), kind)]);
    assert!(!first.exists(), "回滚后已建入口不得残留");
}
