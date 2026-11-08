//! 批量升级测试：落后判定与集合、跳过清单、预检整批不动盘、失败即停、范围收窄。
//! 只看盘上入口、登记簿与报告字段，不看内部动作。

use std::fs;
use std::path::Path;

use super::*;
use crate::hub;

/// 把技能的 frontmatter 版本改成 version 后发一版：
/// 造出「最新 tag 比记录版本新」的盘面，落后判定才有对象
fn release_as(paths: &HubPaths, name: &str, version: &str) {
    fs::write(
        paths.skill_workdir(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 演示\nmetadata:\n  version: \"{version}\"\n---\n\n正文\n"),
    )
    .unwrap();
    crate::skill::release(paths.root(), name, false).unwrap();
}

/// 再入库一个技能：首版 tag 由入库自己打
fn add_skill(paths: &HubPaths, name: &str, version: &str) {
    let src_root = tempfile::tempdir().unwrap();
    let src = src_root.path().join(name);
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: 演示\nmetadata:\n  version: \"{version}\"\n---\n\n正文\n"),
    )
    .unwrap();
    crate::skill::add(paths.root(), &src, None, false).unwrap();
}

#[test]
fn upgrade_all_advances_only_stale_fixed_records() {
    let (_install, paths) = setup_with_skill(Some("0.1.0"));
    let stale = temp_project();
    let pinned = temp_project();
    let following = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        "demo",
        Some("0.1.0"),
        &agents,
        Targets::project(stale.as_path()),
        false,
    )
    .unwrap();
    release_as(&paths, "demo", "1.0.0");
    // 两条不落后的记录：一条钉在最新 tag，一条跟最新
    use_skill(
        paths.root(),
        "demo",
        Some("1.0.0"),
        &agents,
        Targets::project(pinned.as_path()),
        false,
    )
    .unwrap();
    use_skill(
        paths.root(),
        "demo",
        None,
        &agents,
        Targets::project(following.as_path()),
        false,
    )
    .unwrap();

    let r = upgrade_all(paths.root(), None, false, false).unwrap();

    assert!(!r.dry_run);
    assert_eq!(r.upgraded.len(), 1, "只升落后那条: {r:?}");
    let item = &r.upgraded[0];
    assert_eq!(item.key, "demo");
    assert_eq!(item.version.as_deref(), Some("1.0.0"), "升到该技能最新 tag");
    assert_eq!(item.targets[0].from.as_deref(), Some("0.1.0"));
    let kind = item.targets[0].link.expect("实跑要给出实际档位");
    let export = paths.exported("demo", "1.0.0");
    for dest in &item.targets[0].destinations {
        assert!(
            links_to(Path::new(&dest.path), &export, kind),
            "落点应换到最新导出目录: {}",
            dest.path
        );
    }
    assert_eq!(
        record(&paths, stale.as_path(), "demo")
            .unwrap()
            .version
            .as_deref(),
        Some("1.0.0"),
        "登记版本要写回最新"
    );
    // 两条不落后的记录既没进升级清单，也没被动
    assert_eq!(
        record(&paths, pinned.as_path(), "demo")
            .unwrap()
            .version
            .as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        record(&paths, following.as_path(), "demo").unwrap().version,
        None
    );
    let reasons: Vec<&str> = r.skipped.iter().map(|s| s.reason.as_str()).collect();
    assert_eq!(r.skipped.len(), 2, "两条不落后的要逐条报出: {reasons:?}");
    assert!(
        reasons.iter().any(|reason| reason.contains("跟最新")),
        "跟最新的记录要说明原因: {reasons:?}"
    );
    assert!(
        reasons.iter().any(|reason| reason.contains("最新版本")),
        "钉在最新 tag 的记录要说明原因: {reasons:?}"
    );
    assert!(r.stopped.is_none(), "全成功就没有停止点");
}

#[test]
fn upgrade_all_reports_archived_skill_and_dead_project_skips() {
    let (_install, paths) = setup_with_skill(Some("0.1.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    add_skill(&paths, "demo2", "0.1.0");
    use_skill(
        paths.root(),
        "demo",
        Some("0.1.0"),
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    // 登记簿里摆三条不升级的记录：版本比不出大小的、项目路径已不存在的，
    // 以及后面要归档掉的演示技能
    let token = crate::lock::write_lock(&paths).unwrap();
    let mut registry = hub::load_registry(&paths).unwrap();
    registry
        .project_mut(project.as_path())
        .unwrap()
        .skills
        .push(hub::ProjectSkill {
            key: "demo2".into(),
            version: Some("不是版本号".into()),
            agents: agents.to_vec(),
            link: LinkKind::Junction,
        });
    registry.projects.insert(
        "Z:/definitely/not/here".to_string(),
        hub::ProjectRecord {
            last_used: 0,
            skills: vec![hub::ProjectSkill {
                key: "demo".into(),
                version: Some("0.1.0".into()),
                agents: agents.to_vec(),
                link: LinkKind::Junction,
            }],
        },
    );
    hub::save_registry(&paths, &registry, &token).unwrap();
    drop(token);
    // 归档态：工作副本不在、版本 tag 还在（归档拒绝被引用的技能，这里直接摆出那个盘面）
    fs::remove_dir_all(paths.skill_workdir("demo")).unwrap();

    let r = upgrade_all(paths.root(), None, false, true).unwrap();

    assert!(r.upgraded.is_empty(), "这些记录一条都不升: {r:?}");
    assert_eq!(r.skipped.len(), 3, "三条跳过各占一行: {r:?}");
    let dead = r
        .skipped
        .iter()
        .find(|skip| skip.key.is_none())
        .expect("死项目按整条记录报出，没有单个技能键");
    assert_eq!(dead.project.as_deref(), Some("Z:/definitely/not/here"));
    assert!(
        dead.reason.contains("gc"),
        "死记录文案要指向 gc: {}",
        dead.reason
    );
    let archived = r
        .skipped
        .iter()
        .find(|skip| skip.key.as_deref() == Some("demo"))
        .expect("归档技能要报出");
    assert!(
        archived.reason.contains("归档"),
        "归档技能要说明原因: {}",
        archived.reason
    );
    let incomparable = r
        .skipped
        .iter()
        .find(|skip| skip.key.as_deref() == Some("demo2"))
        .expect("版本比不出大小的记录要报出");
    assert!(
        incomparable.reason.contains("判不出落后"),
        "比不出大小就不当落后: {}",
        incomparable.reason
    );
}

#[test]
fn upgrade_all_precheck_failure_touches_nothing_and_names_the_record() {
    let (_install, paths) = setup_with_skill(Some("0.1.0"));
    let first = temp_project();
    let second = temp_project();
    let agents = ["universal".to_string()];
    for project in [&first, &second] {
        use_skill(
            paths.root(),
            "demo",
            Some("0.1.0"),
            &agents,
            Targets::project(project.as_path()),
            false,
        )
        .unwrap();
    }
    release_as(&paths, "demo", "1.0.0");
    let before = registry_text(&paths);
    let kind = record(&paths, first.as_path(), "demo").unwrap().link;
    // 第二条的落点旁预置上次中断的占位残骸：预检点名它，整批不动盘
    let residue = second.join(".agents/skills/.demo.fsh-old");
    fs::create_dir_all(&residue).unwrap();

    let err = upgrade_all(paths.root(), None, false, false).unwrap_err();

    assert_eq!(err.error_code(), "placeholder_residue", "透传底层真因");
    let msg = err.to_string();
    assert!(msg.contains("demo"), "报错要点名记录: {msg}");
    assert!(msg.contains(".demo.fsh-old"), "报错要点名落点: {msg}");
    assert_eq!(registry_text(&paths), before, "预检不合格时登记簿一字不动");
    // 先被检查通过的那条也没被先升掉：预检先于任何盘动作
    for project in [&first, &second] {
        assert_eq!(
            record(&paths, project, "demo").unwrap().version.as_deref(),
            Some("0.1.0")
        );
        assert!(
            links_to(
                &project.join(".agents/skills/demo"),
                &paths.exported("demo", "0.1.0"),
                kind
            ),
            "落点仍指旧源: {}",
            project.display()
        );
    }
}

#[test]
fn upgrade_all_stops_at_first_failure_and_reports_done_and_pending() {
    let (_install, paths) = setup_with_skill(Some("0.1.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    // 一条目标里的三条记录：按登记顺序排成 alpha / zeta / omega
    for name in ["alpha", "zeta", "omega"] {
        add_skill(&paths, name, "0.1.0");
        use_skill(
            paths.root(),
            name,
            Some("0.1.0"),
            &agents,
            Targets::project(project.as_path()),
            false,
        )
        .unwrap();
    }
    for name in ["alpha", "zeta", "omega"] {
        release_as(&paths, name, "1.0.0");
    }
    // zeta 新版本的导出目录位置被一个文件占住：预检查不出来，实跑到导出才失败
    fs::write(paths.exported("zeta", "1.0.0"), "blocked").unwrap();

    let r = upgrade_all(paths.root(), None, false, false).unwrap();

    assert_eq!(r.upgraded.len(), 1, "失败前只完成了一条: {r:?}");
    assert_eq!(r.upgraded[0].key, "alpha");
    let stop = r.stopped.expect("任一条失败即停");
    assert_eq!(stop.key, "zeta", "点名失败的那条");
    assert!(!stop.code.is_empty(), "报告里要给稳定错误码");
    assert_eq!(stop.not_started.len(), 1, "omega 未开始");
    assert!(
        stop.not_started[0].contains("omega"),
        "未开始清单点名记录: {:?}",
        stop.not_started
    );
    // 已完成的那条留在最新，失败那条回到旧版本，未开始那条一字未动
    assert_eq!(
        record(&paths, project.as_path(), "alpha")
            .unwrap()
            .version
            .as_deref(),
        Some("1.0.0")
    );
    for name in ["zeta", "omega"] {
        assert_eq!(
            record(&paths, project.as_path(), name)
                .unwrap()
                .version
                .as_deref(),
            Some("0.1.0"),
            "{name} 不得停在半截"
        );
    }
}

#[test]
fn upgrade_all_dry_run_lists_old_to_latest_and_touches_nothing() {
    let (_install, paths) = setup_with_skill(Some("0.1.0"));
    let project = temp_project();
    let agents = ["universal".to_string()];
    use_skill(
        paths.root(),
        "demo",
        Some("0.1.0"),
        &agents,
        Targets::project(project.as_path()),
        false,
    )
    .unwrap();
    release_as(&paths, "demo", "1.0.0");
    let before = registry_text(&paths);
    let kind = record(&paths, project.as_path(), "demo").unwrap().link;

    let r = upgrade_all(paths.root(), None, false, true).unwrap();

    assert!(r.dry_run);
    assert_eq!(r.upgraded.len(), 1);
    let joined = r.upgraded[0].targets[0].actions.join("\n");
    assert!(
        joined.contains("0.1.0") && joined.contains("@1.0.0"),
        "预演要打出旧版本 → 最新: {joined}"
    );
    assert!(
        !paths.exported("demo", "1.0.0").exists(),
        "预演不得导出新版本"
    );
    assert!(
        links_to(
            &project.join(".agents/skills/demo"),
            &paths.exported("demo", "0.1.0"),
            kind
        ),
        "预演不得换靶"
    );
    assert_eq!(registry_text(&paths), before, "预演不写登记簿");
}

#[test]
fn upgrade_all_scope_narrows_to_named_targets() {
    let (_install, paths) = setup_with_skill(Some("0.1.0"));
    let first = temp_project();
    let second = temp_project();
    let agents = ["universal".to_string()];
    for project in [&first, &second] {
        use_skill(
            paths.root(),
            "demo",
            Some("0.1.0"),
            &agents,
            Targets::project(project.as_path()),
            false,
        )
        .unwrap();
    }
    let home = tempfile::tempdir().unwrap();
    let agent = global_agent(&paths, home.path());
    use_skill(
        paths.root(),
        "demo",
        Some("0.1.0"),
        &[agent],
        Targets::global(),
        false,
    )
    .unwrap();
    release_as(&paths, "demo", "1.0.0");

    // 缺省全 hub：项目段与全局段都在范围里
    let all = upgrade_all(paths.root(), None, false, true).unwrap();
    assert_eq!(all.upgraded.len(), 3, "缺省扫全 hub: {all:?}");
    // 只收全局
    let global_only = upgrade_all(paths.root(), Some(Targets::global()), false, true).unwrap();
    assert_eq!(global_only.upgraded.len(), 1);
    assert_eq!(global_only.upgraded[0].targets[0].scope, hub::Scope::Global);
    // 只收一个项目
    let one = upgrade_all(
        paths.root(),
        Some(Targets::project(first.as_path())),
        false,
        true,
    )
    .unwrap();
    assert_eq!(one.upgraded.len(), 1);
    assert_eq!(one.upgraded[0].targets[0].scope, hub::Scope::Project);
    // 实跑同样只动点名的目标
    upgrade_all(
        paths.root(),
        Some(Targets::project(second.as_path())),
        false,
        false,
    )
    .unwrap();
    assert_eq!(
        record(&paths, second.as_path(), "demo")
            .unwrap()
            .version
            .as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        record(&paths, first.as_path(), "demo")
            .unwrap()
            .version
            .as_deref(),
        Some("0.1.0"),
        "没点名的项目不动"
    );
}
