//! 版本切换测试：换靶与三闸、幂等命中、整批还原、占位残留与落点缺失。

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::hub;

/// 一个技能目录里现存的条目名：占位残骸会在这里露出来
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn upgrade_switches_record_and_destinations_to_fixed_version() {
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

    let r = upgrade(
        paths.root(),
        "demo",
        Some("1.0.0"),
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap();

    assert_eq!(r.version.as_deref(), Some("1.0.0"));
    let target = &r.targets[0];
    assert_eq!(target.from, None, "原来是跟最新");
    assert!(!target.idempotent, "换了版本就不是幂等命中");
    assert_eq!(target.agents, agents, "版本切换不改 agent 名单");
    assert_eq!(target.destinations.len(), 2);
    let kind = target.link.expect("实跑必须给出换后的链接方式");
    let export = paths.exported("demo", "1.0.0");
    assert!(export.join("SKILL.md").is_file(), "固定版本应导出");
    for dest in &target.destinations {
        assert!(
            links_to(Path::new(&dest.path), &export, kind),
            "落点应链到导出目录: {}",
            dest.path
        );
    }
    // 占位只活到全部落点换完：技能目录里不该再有任何残骸
    assert_eq!(
        entries(&project.join(".agents/skills")),
        vec!["demo".to_string()]
    );
    assert_eq!(
        entries(&project.join(".dsh/skills")),
        vec!["demo".to_string()]
    );

    let rec = record(&paths, project.as_path(), "demo").expect("登记簿应有记录");
    assert_eq!(rec.version.as_deref(), Some("1.0.0"));
    assert_eq!(rec.link, kind, "实际档位要写回登记簿");
    assert_eq!(rec.agents, agents);
    assert!(r.cleanup.is_some(), "换过版本后应报告顺手回收统计");
}

#[test]
fn upgrade_back_to_latest_relinks_workdir() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();

    let r = upgrade(
        paths.root(),
        "demo",
        None,
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap();

    assert_eq!(r.version, None, "不给版本即跟随最新");
    assert_eq!(r.targets[0].from.as_deref(), Some("1.0.0"));
    let kind = r.targets[0].link.expect("实跑必须给出换后的链接方式");
    let dest = PathBuf::from(&r.targets[0].destinations[0].path);
    let workdir = paths.skill_workdir("demo");
    assert!(
        links_to(&dest, &workdir, kind),
        "跟最新的落点应链回工作副本: {}",
        dest.display()
    );
    let rec = record(&paths, project.as_path(), "demo").unwrap();
    assert_eq!(rec.version, None, "记录版本置空");
    // 换出来的旧版本导出目录已无引用：顺手回收清掉，报告带回收统计
    let cleanup = r.cleanup.expect("换过版本应报告顺手回收统计");
    assert!(
        !paths.exported("demo", "1.0.0").exists(),
        "无引用的旧导出目录应被回收"
    );
    assert_eq!(cleanup.removed, 1, "本次回收一个目录: {cleanup:?}");
}

#[test]
fn upgrade_is_idempotent_when_already_at_target() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let project = temp_project();
    let agents = two_agents();
    use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    let before = registry_text(&paths);
    let kind = record(&paths, project.as_path(), "demo").unwrap().link;
    // 两个探针：登记簿写盘路径被占住（真去写就报 io）、一个无引用导出目录（真去回收就没了）
    fs::create_dir_all(paths.projects_toml().with_extension("toml.tmp")).unwrap();
    let orphan = paths.exported("demo", "0.0.1");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(orphan.join("stale.txt"), "stale").unwrap();

    let r = upgrade(
        paths.root(),
        "demo",
        Some("1.0.0"),
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap();

    assert!(r.targets[0].idempotent, "已是目标版本应标为幂等命中");
    assert!(
        orphan.join("stale.txt").is_file(),
        "幂等命中不得触发顺手回收"
    );
    assert!(r.cleanup.is_none(), "没有盘动作就没有回收统计");
    assert!(r.targets[0].actions.is_empty(), "实跑不带动作清单");
    assert_eq!(
        r.targets[0].link,
        Some(kind),
        "幂等命中报出的是未变动的登记档位"
    );
    assert_eq!(registry_text(&paths), before, "幂等命中不得写登记簿");
    let export = paths.exported("demo", "1.0.0");
    for dest in &r.targets[0].destinations {
        assert!(links_to(Path::new(&dest.path), &export, kind));
    }
    assert_eq!(
        entries(&project.join(".agents/skills")),
        vec!["demo".to_string()],
        "幂等命中不得落任何盘动作"
    );
}

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
fn registry_write_failure_restores_old_entries() {
    let (_install, paths) = setup_with_skill(Some("1.0.0"));
    let first = temp_project();
    let second = temp_project();
    let agents = ["universal".to_string()];
    for project in [&first, &second] {
        use_skill(
            paths.root(),
            "demo",
            None,
            &agents,
            Targets::project(project.as_path()),
            false,
        )
        .unwrap();
    }
    let before = registry_text(&paths);
    let kind = record(&paths, first.as_path(), "demo").unwrap().link;
    // 登记簿的临时文件位置被目录占住：换靶全部做完之后写盘才失败
    fs::create_dir_all(paths.projects_toml().with_extension("toml.tmp")).unwrap();

    let err = upgrade(
        paths.root(),
        "demo",
        Some("1.0.0"),
        Targets::new(false, vec![first.clone(), second.clone()]),
        false,
        false,
    )
    .unwrap_err();

    assert_eq!(err.error_code(), "io");
    for project in [&first, &second] {
        let dest = project.join(".agents/skills/demo");
        assert!(
            links_to(&dest, &paths.skill_workdir("demo"), kind),
            "整批还原后旧入口应回到原名并仍指旧源: {}",
            dest.display()
        );
        assert_eq!(
            entries(&project.join(".agents/skills")),
            vec!["demo".to_string()],
            "不得留下占位或本次新建的入口"
        );
    }
    assert_eq!(registry_text(&paths), before, "写盘失败时登记簿一字不动");
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
fn upgrade_reports_copy_not_following_for_idempotent_copy_record() {
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
    // 手工造一条拷贝档记录：落点是真目录，登记档位写 copy
    let dest = project.join(".agents/skills/demo");
    let registered = record(&paths, project.as_path(), "demo").unwrap();
    crate::link::remove_link(&dest, registered.link).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::copy(
        paths.skill_workdir("demo").join("SKILL.md"),
        dest.join("SKILL.md"),
    )
    .unwrap();
    let mut registry = hub::load_registry(&paths).unwrap();
    let token = crate::lock::write_lock(&paths).unwrap();
    for skill in registry
        .project_mut(project.as_path())
        .unwrap()
        .skills
        .iter_mut()
        .filter(|s| s.key == "demo")
    {
        skill.link = LinkKind::Copy;
    }
    hub::save_registry(&paths, &registry, &token).unwrap();
    // 放开写锁再版本切换：否则本次调用会被自己的锁挡在门外
    drop(token);

    let r = upgrade(
        paths.root(),
        "demo",
        None,
        Targets::project(project.as_path()),
        false,
        false,
    )
    .unwrap();

    assert!(r.targets[0].idempotent, "拷贝档跟最新同样没有盘动作");
    let note = r.targets[0].note.as_deref().expect("拷贝档要给出提醒");
    assert!(
        note.contains("拷贝") && note.contains("重新铺"),
        "提醒要说明拷贝不跟随、刷新要重铺: {note}"
    );
}

#[test]
fn upgrade_dry_run_lists_swap_actions_and_touches_nothing() {
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

    let r = upgrade(
        paths.root(),
        "demo",
        Some("1.0.0"),
        Targets::project(project.as_path()),
        false,
        true,
    )
    .unwrap();

    assert!(r.dry_run);
    let actions = &r.targets[0].actions;
    let joined = actions.join("\n");
    assert!(
        joined.contains("跟最新") && joined.contains("@1.0.0"),
        "预演要打出旧版本 → 新版本: {joined}"
    );
    for verb in ["rename", "link", "remove", "register"] {
        assert!(
            actions.iter().any(|a| a.starts_with(verb)),
            "预演要列出 {verb} 这一步: {joined}"
        );
    }
    assert!(joined.contains(".fsh-old"), "预演要列出占位名: {joined}");
    assert!(!paths.exported("demo", "1.0.0").exists(), "预演不得导出");
    let dest = project.join(".agents/skills/demo");
    assert!(
        links_to(&dest, &paths.skill_workdir("demo"), kind),
        "预演不得换靶"
    );
    assert_eq!(registry_text(&paths), before, "预演不写登记簿");
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
