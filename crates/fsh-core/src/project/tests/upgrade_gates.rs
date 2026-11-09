//! 版本切换的预检拒绝：前导校验与落点三闸，任一不合格整次不动盘。

use std::fs;

use super::*;

#[test]
fn upgrade_rejects_placeholder_residue_without_touching_disk() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    let before = registry_text(&paths);
    let kind = record(&paths, project.as_path(), "demo").unwrap().link;
    // 上次中断的残骸：占位名被一个目录占住，工具只点名、不代删
    let residue = project.join(".agents/skills/.demo.fsh-old");
    fs::create_dir_all(&residue).unwrap();
    fs::write(residue.join("left-over.txt"), "keep").unwrap();

    let err = upgrade(
        paths.root(),
        "demo",
        Some("1.0.0"),
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap_err();

    assert_eq!(err.error_code(), "placeholder_residue");
    let msg = err.to_string();
    assert!(msg.contains(".demo.fsh-old"), "报错要点名完整路径: {msg}");
    assert!(residue.join("left-over.txt").is_file(), "残骸绝不自动删");
    let dest = project.join(".agents/skills/demo");
    assert!(
        links_to(&dest, &paths.skill_workdir("demo"), kind),
        "拒绝时旧入口原样留着"
    );
    assert_eq!(registry_text(&paths), before, "拒绝时登记簿一字不动");
}

#[test]
fn upgrade_reports_missing_destination_with_both_ways_out() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = two_agents();
    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    let before = registry_text(&paths);
    let kind = record(&paths, project.as_path(), "demo").unwrap().link;
    // 落点父路径被同名文件占住：这一处落点已经不存在，另一处还在
    crate::link::remove_link(&project.join(".dsh/skills/demo"), kind).unwrap();
    fs::remove_dir_all(project.join(".dsh")).unwrap();
    fs::write(project.join(".dsh"), "not a dir").unwrap();

    let err = upgrade(
        paths.root(),
        "demo",
        Some("1.0.0"),
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap_err();

    assert_eq!(err.error_code(), "destination_missing");
    let msg = err.to_string();
    // 报告里的落点是规范化绝对路径，比对也要走规范化形态
    let gone_named = fs::canonicalize(&project).unwrap().join(".dsh/skills/demo");
    assert!(
        msg.contains(&gone_named.display().to_string()),
        "点名落点: {msg}"
    );
    assert!(msg.contains("dsh"), "点名 agent: {msg}");
    assert!(msg.contains("unuse"), "给出按 agent 撤回的出路: {msg}");
    assert!(msg.contains("fsh use"), "给出重新使用的出路: {msg}");
    // 另一处落点原样留着，登记簿不动
    assert!(links_to(
        &project.join(".agents/skills/demo"),
        &paths.skill_workdir("demo"),
        kind
    ));
    assert_eq!(registry_text(&paths), before);
}

#[test]
fn upgrade_protects_modified_copy_unless_forced() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    let r = use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    let dest = project.join(".agents/skills/demo");
    if r.targets[0].link == Some(LinkKind::Copy) {
        // 拷贝档：手改内容默认拒绝版本切换，--force 只覆盖这一项
        fs::write(dest.join("SKILL.md"), "手改内容").unwrap();
        let err = upgrade(
            paths.root(),
            "demo",
            Some("1.0.0"),
            Targets::project(project.as_path()),
            false,
            false,
        )
        .unwrap_err();
        assert_eq!(err.error_code(), "modified_copy");
        assert!(
            fs::read_to_string(dest.join("SKILL.md"))
                .unwrap()
                .contains("手改内容"),
            "拒绝时手改内容不得被销毁"
        );
        upgrade(
            paths.root(),
            "demo",
            Some("1.0.0"),
            Targets::project(project.as_path()),
            true,
            false,
        )
        .unwrap();
        assert_eq!(
            record(&paths, project.as_path(), "demo")
                .unwrap()
                .version
                .as_deref(),
            Some("1.0.0")
        );
    } else {
        // 链接档：落点被换成普通目录时必须拒绝，force 也不覆盖形态闸
        crate::link::remove_link(&dest, r.targets[0].link.unwrap()).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("user.txt"), "keep").unwrap();
        for force in [false, true] {
            let err = upgrade(
                paths.root(),
                "demo",
                Some("1.0.0"),
                Targets::project(project.as_path()),
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
fn upgrade_requires_registered_record_and_known_version() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    // 没有登记记录：版本切换只改已有记录
    let err = upgrade(
        paths.root(),
        "demo",
        Some("1.0.0"),
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "not_registered");

    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    // 版本不存在
    let err = upgrade(
        paths.root(),
        "demo",
        Some("9.9.9"),
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "version_not_found");
    // 技能不存在
    let err = upgrade(
        paths.root(),
        "ghost",
        None,
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "skill_not_found");
    // 归档技能不能用：工作副本没了、只留版本 tag 与登记记录（hub 被手工改动时的防御）
    fs::remove_dir_all(paths.skill_workdir("demo")).unwrap();
    let err = upgrade(
        paths.root(),
        "demo",
        None,
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "archived");
    // 没有目标
    let err = upgrade(
        paths.root(),
        "demo",
        None,
        Targets::new(false, Vec::new()),
        false,
        false,
    )
    .unwrap_err();
    assert_eq!(err.error_code(), "no_target");
}
