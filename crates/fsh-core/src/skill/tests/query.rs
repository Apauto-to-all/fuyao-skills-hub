//! 查询测试：列表、详情、按文件 diff、归档态从标签树读 frontmatter。

use std::fs;

use super::*;

#[test]
fn list_shows_rows_with_archive_flag_and_tag_filter() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "alpha", None, "甲"),
        None,
        false,
    )
    .unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "beta", None, "乙"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "beta", false).unwrap();
    // 给 alpha 补 source/tags：来源与标签只有 frontmatter 一处，改文件即可
    fs::write(
        paths.skill_workdir("alpha").join("SKILL.md"),
        "---\nname: alpha\ndescription: 甲\nmetadata:\n  version: \"0.1.0\"\n  source: somewhere\n  tags: \"rust\"\n---\n\n# alpha\n正文\n",
    )
    .unwrap();

    let rows = list(&root(&install), None).unwrap();
    let alpha = rows.iter().find(|r| r.key == "alpha").unwrap();
    let beta = rows.iter().find(|r| r.key == "beta").unwrap();
    assert_eq!(alpha.latest_version.as_deref(), Some("0.1.0"));
    assert!(!alpha.archived);
    assert_eq!(alpha.source.as_deref(), Some("somewhere"));
    assert_eq!(alpha.description.as_deref(), Some("甲"));
    assert_eq!(alpha.tags, vec!["rust"]);
    assert!(
        alpha.updated.is_some(),
        "最新版本的发布提交时间应带出 updated"
    );
    assert!(beta.archived, "归档后 list 应标 archived");
    assert_eq!(beta.source, None, "没写 source 的行就是 None");
    assert_eq!(beta.description.as_deref(), Some("乙"));
    assert!(beta.tags.is_empty(), "没写 tags 的行就是空清单");
    // tag 过滤
    let filtered = list(&root(&install), Some("rust")).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].key, "alpha");
    let none = list(&root(&install), Some("nope")).unwrap();
    assert!(none.is_empty());
}

#[test]
fn show_returns_full_detail_with_references() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "full", None, "全量详情"),
        None,
        false,
    )
    .unwrap();
    // 登记簿记一条引用（用法的唯一记录）
    let proj = tempfile::tempdir().unwrap();
    register_usage(&paths, proj.path(), "full", Some("0.1.0"));

    let d = show(&root(&install), "full").unwrap();
    assert_eq!(d.latest_version.as_deref(), Some("0.1.0"));
    assert!(!d.archived);
    assert_eq!(d.timeline.len(), 1);
    assert_eq!(d.timeline[0].version, "0.1.0");
    assert_eq!(d.timeline[0].commit.len(), 7, "短哈希七位");
    assert!(d.timeline[0].date > 0, "发布时间取提交时间");
    let fm = d.frontmatter.as_ref().expect("活跃态应读到 frontmatter");
    assert_eq!(fm.description, "全量详情");
    assert_eq!(fm.version.as_deref(), Some("0.1.0"));
    assert_eq!(d.usage.len(), 1, "一条登记记录就是引用视图的一行");
    assert_eq!(d.usage[0].scope, hub::Scope::Project);
    assert_eq!(
        d.usage[0].project.as_deref(),
        Some(proj.path().display().to_string().as_str()),
        "项目记录带出展示路径"
    );
    assert_eq!(d.usage[0].agents, vec!["universal".to_string()]);
    assert_eq!(
        d.usage[0].version.as_deref(),
        Some("0.1.0"),
        "固定版本的记录带出登记的版本"
    );
    assert_eq!(d.destination_count, Some(1), "登记簿一条引用解析出一个落点");
    let content = d.content.as_deref().expect("详情应带出 SKILL.md 原文");
    assert!(content.contains("# full"), "预览内容应是 SKILL.md 原文");
    // 解引用后才可归档；归档后仍可 show（frontmatter 读自最新 tag 树）
    hub::save_registry(
        &paths,
        &hub::ProjectRegistry::default(),
        &crate::lock::write_lock(&paths).unwrap(),
    )
    .unwrap();
    archive(&root(&install), "full", false).unwrap();
    let d2 = show(&root(&install), "full").unwrap();
    assert!(d2.archived);
    assert_eq!(
        d2.frontmatter.as_ref().map(|f| f.description.as_str()),
        Some("全量详情")
    );
    assert!(matches!(
        show(&root(&install), "ghost"),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn show_lists_each_usage_record_with_scope_and_registered_version() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "mixed", None, "两段都在用"),
        None,
        false,
    )
    .unwrap();
    let proj = tempfile::tempdir().unwrap();
    register_usage(&paths, proj.path(), "mixed", None);
    register_global_usage(&paths, "mixed", &["universal", "dsh"]);

    let d = show(&root(&install), "mixed").unwrap();
    assert_eq!(d.usage.len(), 2, "项目一条、全局一条，各占一行");
    // 项目记录在前、全局记录在后，与登记簿两段顺序一致
    assert_eq!(d.usage[0].scope, hub::Scope::Project);
    assert_eq!(
        d.usage[0].project.as_deref(),
        Some(proj.path().display().to_string().as_str())
    );
    assert_eq!(d.usage[0].agents, vec!["universal".to_string()]);
    assert_eq!(d.usage[0].version, None, "跟最新的记录版本如实为空");
    assert_eq!(d.usage[1].scope, hub::Scope::Global);
    assert_eq!(d.usage[1].project, None, "全局记录没有项目");
    assert_eq!(
        d.usage[1].agents,
        vec!["universal".to_string(), "dsh".to_string()],
        "agent 名单按登记原样带出"
    );
    assert_eq!(d.usage[1].version, None);
}

#[test]
fn diff_reports_per_file_unified_patch() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "delta", None, "差异");
    add(&root(&install), &src, None, false).unwrap();
    // 改 SKILL.md + 新增文件，发 0.2.0
    fs::write(
        paths.skill_workdir("delta").join("SKILL.md"),
        "---\nname: delta\ndescription: 差异\nmetadata:\n  version: \"0.2.0\"\n---\n\n新正文\n",
    )
    .unwrap();
    fs::write(paths.skill_workdir("delta").join("NEW.md"), "新文件").unwrap();
    release(
        &root(&install),
        "delta",
        ReleaseSource::Frontmatter,
        false,
        false,
    )
    .unwrap();

    let files = diff(&root(&install), "delta", "0.1.0", "0.2.0").unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(names.contains(&"NEW.md"));
    assert!(names.contains(&"SKILL.md"));
    let skill_patch = files.iter().find(|f| f.path == "SKILL.md").unwrap();
    assert!(skill_patch.patch.contains("-正文"));
    assert!(skill_patch.patch.contains("+新正文"));
    // 同版本 diff 为空
    assert!(
        diff(&root(&install), "delta", "0.2.0", "0.2.0")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn archived_skill_reads_frontmatter_from_tag_tree() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "arch", Some("1.2.0"), "归档也读得到");
    fs::write(
        src.join("SKILL.md"),
        "---\nname: arch\ndescription: 归档也读得到\nmetadata:\n  version: \"1.2.0\"\n  source: owner/repo\n  tags: \"tdd, office\"\n---\n\n# arch\n正文\n",
    )
    .unwrap();
    add(&root(&install), &src, None, false).unwrap();
    archive(&root(&install), "arch", false).unwrap();

    // list --tag 对归档态照样可用：来源与标签从最新 tag 树读
    let filtered = list(&root(&install), Some("office")).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].key, "arch");
    assert!(filtered[0].archived);
    assert_eq!(filtered[0].source.as_deref(), Some("owner/repo"));
    assert!(list(&root(&install), Some("nope")).unwrap().is_empty());

    // show 给出 frontmatter 全字段
    let d = show(&root(&install), "arch").unwrap();
    assert!(d.archived);
    let fm = d.frontmatter.expect("归档态应能从 tag 树读到 frontmatter");
    assert_eq!(fm.name, "arch");
    assert_eq!(fm.description, "归档也读得到");
    assert_eq!(fm.version.as_deref(), Some("1.2.0"));
    assert_eq!(fm.source.as_deref(), Some("owner/repo"));
    assert_eq!(fm.tags, vec!["tdd", "office"]);
    assert!(!paths.root().join("meta").exists());
}

/// frontmatter 形状错位：list 照旧降级成空值，show 带出失败原因点名文件与字段
#[test]
fn show_reports_frontmatter_error_while_list_degrades() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "broken", None, "形状错位"),
        None,
        false,
    )
    .unwrap();
    let work = paths.skill_workdir("broken").join("SKILL.md");
    // fsh 自有字段平铺在顶层：形状错位，解析必然失败
    fs::write(
        &work,
        "---\nname: broken\ndescription: 形状错位\nversion: \"0.2.0\"\n---\n\n正文\n",
    )
    .unwrap();

    let rows = list(&root(&install), None).unwrap();
    let row = rows.iter().find(|r| r.key == "broken").unwrap();
    assert_eq!(row.description, None, "list 照旧降级成空值");
    assert!(row.tags.is_empty(), "降级的行没有标签");

    let d = show(&root(&install), "broken").unwrap();
    assert!(d.frontmatter.is_none());
    let reason = d
        .frontmatter_error
        .as_deref()
        .expect("show 应带出解析失败原因");
    assert!(
        reason.contains("version") && reason.contains("metadata"),
        "原因要点名字段与正确位置: {reason}"
    );
    assert!(
        reason.contains("store/broken/SKILL.md"),
        "原因要点名文件: {reason}"
    );

    // 改回新形状：解析成功，失败原因消失
    fs::write(
        &work,
        "---\nname: broken\ndescription: 形状正确\nmetadata:\n  version: \"0.2.0\"\n---\n\n正文\n",
    )
    .unwrap();
    let fixed = show(&root(&install), "broken").unwrap();
    assert!(fixed.frontmatter_error.is_none(), "解析成功时没有失败原因");
    assert_eq!(
        fixed.frontmatter.map(|fm| fm.description),
        Some("形状正确".into())
    );
}
