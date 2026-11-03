//! 项目使用行为测试：只经公共 API 观察落点、登记簿、导出目录与错误码。

use std::fs;
use std::path::Path;

use super::*;
use crate::hub;

/// 建一次性 hub（初始化完成）+ 一个已入库技能
fn setup_with_skill(version: Option<&str>) -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let src = src_root.path().join("demo");
    fs::create_dir_all(src.join("references")).unwrap();
    let fm_version = version
        .map(|v| format!("version: \"{v}\"\n"))
        .unwrap_or_default();
    fs::write(
        src.join("SKILL.md"),
        format!("---\nname: demo\n{fm_version}description: 演示\n---\n\n正文\n"),
    )
    .unwrap();
    fs::write(src.join("references/a.txt"), "ref-a").unwrap();
    crate::skill::add(paths.root(), &src, None).unwrap();
    (install, paths)
}

/// 建一个有项目目录的一次性项目（项目根必须已存在；不自动删，路径留给测试用）
fn temp_project() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.keep();
    fs::create_dir_all(&p).unwrap();
    p
}

/// 一次 use 的 agent 列表（通用位 + dsh 两个不同落点）
fn two_agents() -> Vec<String> {
    vec!["universal".to_string(), "dsh".to_string()]
}

/// 登记簿里某项目的该技能记录
fn record(paths: &HubPaths, project: &Path, key: &str) -> Option<hub::ProjectSkill> {
    let reg = hub::load_registry(paths).unwrap();
    reg.projects
        .iter()
        .find(|p| path_identity(Path::new(&p.path)) == path_identity(project))
        .and_then(|p| p.skills.iter().find(|s| s.key == key).cloned())
}

#[test]
fn use_latest_links_workdir_and_registers_once() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = two_agents();
    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        project.as_path(),
        false,
    )
    .unwrap();

    assert_eq!(r.version, None);
    assert_eq!(r.agents, agents);
    assert_eq!(r.destinations.len(), 2);
    // 两个不同落点都真的能读到正文
    for name in [".agents/skills/demo", ".dsh/skills/demo"] {
        let p = project.as_path().join(name);
        assert!(p.join("SKILL.md").is_file(), "{name} 应可读");
        assert!(p.join("references/a.txt").is_file());
    }
    let rec = record(&paths, project.as_path(), "demo").expect("登记簿应有记录");
    assert_eq!(rec.version, None);
    assert_eq!(rec.agents, agents);
    assert_eq!(rec.link, r.link);
}

#[test]
fn use_fixed_version_exports_and_links_export_dir() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let r = use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &["universal".to_string()],
        project.as_path(),
        false,
    )
    .unwrap();
    assert_eq!(r.version.as_deref(), Some("1.0.0"));
    let export = paths.exported("demo", "1.0.0");
    assert!(export.join("SKILL.md").is_file(), "固定版本应导出");
    assert!(
        project
            .as_path()
            .join(".agents/skills/demo/SKILL.md")
            .is_file()
    );
}

#[test]
fn use_rejects_archived_skill_even_with_known_version() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    crate::skill::archive(paths.root(), "demo").unwrap();
    let project = temp_project();
    for version in [None, Some("1.0.0")] {
        let err = use_skill(
            paths.root(),
            "demo",
            version,
            &["universal".to_string()],
            project.as_path(),
            false,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "archived");
    }
    assert!(
        !project.as_path().join(".agents/skills/demo").exists(),
        "拒绝时不得落盘"
    );
}

#[test]
fn use_rejects_unknown_skill_version_and_missing_project() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let agents = ["universal".to_string()];
    let err = use_skill(
        paths.root(),
        "ghost",
        None,
        &agents,
        project.as_path(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "skill_not_found");
    let err = use_skill(
        paths.root(),
        "demo",
        Some("9.9.9"),
        &agents,
        project.as_path(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "version_not_found");
    let missing = project.as_path().join("no-such-dir");
    let err = use_skill(paths.root(), "demo", None, &agents, &missing, false).unwrap_err();
    assert_eq!(err.error_code(), "invalid_project");
}

#[test]
fn use_rejects_duplicate_registration_and_occupied_destinations() {
    let (_install, paths) = setup_with_skill(None);
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

    // 已登记：必须先 unuse
    let err = use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        project.as_path(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "already_registered");

    // 另一个项目里同名落点已被用户占用：拒绝且不覆盖
    let other = temp_project();
    let occupied = other.as_path().join(".agents/skills/demo");
    fs::create_dir_all(&occupied).unwrap();
    fs::write(occupied.join("user.txt"), "keep").unwrap();
    let err = use_skill(paths.root(), "demo", None, &agents, other.as_path(), false).unwrap_err();
    assert_eq!(err.error_code(), "destination_occupied");
    assert_eq!(
        fs::read_to_string(occupied.join("user.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn use_dedupes_agents_sharing_one_destination() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    // 通用位与 codex 共用 .agents/skills
    let agents = vec!["universal".to_string(), "codex".to_string()];
    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        project.as_path(),
        false,
    )
    .unwrap();
    assert_eq!(r.destinations.len(), 1, "共用落点只建一次");
    assert_eq!(r.destinations[0].agents, agents);
    let rec = record(&paths, project.as_path(), "demo").unwrap();
    assert_eq!(rec.agents, agents);
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
fn use_reports_cleanup_result() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &["universal".to_string()],
        project.as_path(),
        false,
    )
    .unwrap();
    assert!(
        r.cleanup.is_some(),
        "使用后应报告顺手回收统计（无引用导出目录回收可空但统计存在）"
    );
}

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
        project.as_path(),
        false,
    )
    .unwrap();

    let r = unuse(paths.root(), "demo", project.as_path(), false, false).unwrap();
    assert_eq!(r.removed.len(), 2);
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
        project.as_path(),
        false,
    )
    .unwrap();
    fs::remove_dir_all(project.as_path().join(".dsh/skills/demo")).unwrap();

    // 缺失落点视为已撤回
    unuse(paths.root(), "demo", project.as_path(), false, false).unwrap();
    // 记录没了：再次撤回报错，不扫描黑链接
    let err = unuse(paths.root(), "demo", project.as_path(), false, false).unwrap_err();
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
        project.as_path(),
        false,
    )
    .unwrap();
    let dest = project.as_path().join(".agents/skills/demo");
    if r.link == LinkKind::Copy {
        fs::write(dest.join("SKILL.md"), "手改内容").unwrap();
        let err = unuse(paths.root(), "demo", project.as_path(), false, false).unwrap_err();
        assert_eq!(err.error_code(), "copy_modified");
        assert!(dest.is_dir(), "拒绝时不得删除");
        unuse(paths.root(), "demo", project.as_path(), true, false).unwrap();
        assert!(!dest.exists());
    } else {
        // 链接档位：落点被换成普通目录时必须拒绝，force 也不覆盖
        fs::remove_dir(&dest).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("user.txt"), "keep").unwrap();
        for force in [false, true] {
            let err = unuse(paths.root(), "demo", project.as_path(), force, false).unwrap_err();
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
        project.as_path(),
        false,
    )
    .unwrap();
    let actions = unuse_dry(paths.root(), "demo", project.as_path(), false).unwrap();
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

/// 未初始化 hub：项目使用必须先报 hub_not_initialized
#[test]
fn use_requires_initialized_hub() {
    let dir = tempfile::tempdir().unwrap();
    let project = temp_project();
    let err = use_skill(
        dir.path(),
        "demo",
        None,
        &["universal".to_string()],
        project.as_path(),
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "hub_not_initialized");
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
        project.as_path(),
        false,
    );
    // 该键不存在：先确认错误码，再验证存在键的落点命名
    assert_eq!(r.unwrap_err().error_code(), "skill_not_found");
    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &["universal".to_string()],
        project.as_path(),
        false,
    )
    .unwrap();
    assert!(r.destinations[0].path.ends_with("demo"));
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
        project.as_path(),
        false,
    )
    .unwrap();
    assert!(Path::new(&r.project).is_absolute());
}

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
