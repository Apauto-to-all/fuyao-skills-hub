use super::*;
use std::path::PathBuf;

/// 建一次性 hub（初始化完成、工作区干净）
fn setup() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    (install, paths)
}

/// hub 根路径（install\data）
fn root(install: &tempfile::TempDir) -> PathBuf {
    install.path().join("data")
}

/// 造一个源 skill 目录（带 frontmatter）
fn make_skill(dir: &Path, name: &str, version: Option<&str>, desc: &str) -> PathBuf {
    let skill_dir = dir.join(name);
    fs::create_dir_all(skill_dir.join("references")).unwrap();
    let fm_version = version
        .map(|v| format!("version: \"{v}\"\n"))
        .unwrap_or_default();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\n{fm_version}description: {desc}\n---\n\n# {name}\n正文\n"),
    )
    .unwrap();
    fs::write(skill_dir.join("references").join("a.txt"), "ref-a").unwrap();
    skill_dir
}

/// 改工作副本 frontmatter 里的号（发版输入是文件，fsh 只读不代算）
fn set_version(paths: &HubPaths, key: &str, version: &str) {
    let file = paths.skill_workdir(key).join("SKILL.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, set_frontmatter_version(&text, version).unwrap()).unwrap();
}

/// 在登记簿里记一条项目引用（use 之后的状态）
fn register_usage(paths: &HubPaths, project: &Path, key: &str, version: Option<&str>) {
    let mut reg = hub::load_registry(paths).unwrap();
    reg.upsert(project, 1);
    reg.projects[0].skills.push(hub::ProjectSkill {
        key: key.into(),
        version: version.map(str::to_string),
        agents: vec!["agents".into()],
        link: crate::link::LinkKind::Junction,
    });
    hub::save_registry(paths, &reg, &crate::lock::write_lock(paths).unwrap()).unwrap();
}

#[test]
fn add_creates_workdir_and_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例技能");

    let report = add(&root(&install), &src, None).unwrap();
    assert_eq!(
        report,
        AddReport {
            key: "demo".into(),
            version: "0.1.0".into()
        }
    );
    // 工作副本落盘：SKILL.md 就是元数据唯一住所
    assert!(paths.skill_workdir("demo").join("SKILL.md").is_file());
    // 缺省的号要落回编辑面：号只有一个诞生地（文件）
    let work = fs::read_to_string(paths.skill_workdir("demo").join("SKILL.md")).unwrap();
    assert!(
        work.contains("version: \"0.1.0\""),
        "缺省首版号应写回: {work}"
    );
    assert!(!paths.root().join("meta").exists(), "meta 目录不应存在");
    // 首版 tag 就位；入库不产导出目录，导出由 use 按需生成
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "demo").unwrap(), vec!["0.1.0"]);
    assert!(!paths.exported("demo", "0.1.0").exists());
    // 提交完成、工作区干净
    assert!(!git::is_dirty(&repo).unwrap());
}

#[test]
fn add_honors_frontmatter_version() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "ver-skill", Some("2.1.0"), "带版本");
    let report = add(&root(&install), &src, None).unwrap();
    assert_eq!(report.version, "2.1.0");
}

#[test]
fn add_rejects_invalid_sources() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    // 无 SKILL.md
    let empty = src_dir.path().join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert!(matches!(
        add(&root(&install), &empty, None),
        Err(SkillError::InvalidSource { .. })
    ));
    // SKILL.md 无 frontmatter
    let no_fm = src_dir.path().join("no-fm");
    fs::create_dir_all(&no_fm).unwrap();
    fs::write(no_fm.join("SKILL.md"), "# 只有正文\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_fm, None),
        Err(SkillError::InvalidSource { .. })
    ));
    // frontmatter 无 name
    let no_name = src_dir.path().join("no-name");
    fs::create_dir_all(&no_name).unwrap();
    fs::write(no_name.join("SKILL.md"), "---\ndescription: 无名\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_name, None),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn add_rejects_duplicate_by_workdir_or_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "dup", None, "重复");
    add(&root(&install), &src, None).unwrap();
    // 工作副本占位 → 拒绝
    assert!(matches!(
        add(&root(&install), &src, None),
        Err(SkillError::AlreadyExists { .. })
    ));
    // 归档后（无工作副本、有 tag）→ 仍然拒绝
    archive(&root(&install), "dup").unwrap();
    assert!(!paths.skill_workdir("dup").exists());
    assert!(matches!(
        add(&root(&install), &src, None),
        Err(SkillError::AlreadyExists { .. })
    ));
}

#[test]
fn add_excludes_nested_git_dir() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "nested", None, "嵌套仓库");
    fs::create_dir_all(src.join(".git").join("objects")).unwrap();
    fs::write(src.join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
    add(&root(&install), &src, None).unwrap();
    assert!(!paths.skill_workdir("nested").join(".git").exists());
    assert!(
        paths
            .skill_workdir("nested")
            .join("references")
            .join("a.txt")
            .is_file()
    );
}

#[test]
fn release_collects_dirty_changes_into_new_version() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "grow", None, "成长"),
        None,
    )
    .unwrap();
    // 工作副本改脏（编辑面随便改是常态），号写在文件里
    fs::write(
        paths.skill_workdir("grow").join("SKILL.md"),
        "---\nname: grow\ndescription: 成长 v2\nversion: \"0.2.0\"\n---\n\n新版正文\n",
    )
    .unwrap();
    let next = release(&root(&install), "grow").unwrap();
    assert_eq!(next, "0.2.0");
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "grow").unwrap(),
        vec!["0.2.0", "0.1.0"]
    );
    assert!(!git::is_dirty(&repo).unwrap(), "脏改动应被收编进新版本");
    // 新 tag 的 SKILL.md 内容 = 改后版本
    let text = skill_md_from_tag(&repo, "grow/v0.2.0", "grow")
        .unwrap()
        .unwrap();
    assert!(text.contains("新版正文"));
}

#[test]
fn release_rejects_archived_or_missing() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "gone", None, "归档"),
        None,
    )
    .unwrap();
    archive(&root(&install), "gone").unwrap();
    assert!(matches!(
        release(&root(&install), "gone"),
        Err(SkillError::Archived { .. })
    ));
    assert!(matches!(
        release(&root(&install), "never"),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn archive_removes_workdir_but_keeps_versions_and_export() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "arch", None, "归档供版"),
        None,
    )
    .unwrap();
    // 先造一个导出目录，归档要连它一起收
    let exported = paths.exported("arch", "0.1.0");
    fs::create_dir_all(&exported).unwrap();
    fs::write(exported.join("SKILL.md"), "# arch\n").unwrap();

    archive(&root(&install), "arch").unwrap();
    // 编辑面与导出目录删除后，该键只剩下 tag 与 git 历史
    assert!(!paths.skill_workdir("arch").exists());
    assert!(!paths.export().join("arch").exists());
    assert!(!paths.root().join("meta").exists());
    // 版本链全在、历史提交可查
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "arch").unwrap(), vec!["0.1.0"]);
    assert!(!git::is_dirty(&repo).unwrap());
}

#[test]
fn remove_deletes_tags_and_rejects_when_referenced() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "dead", None, "待删"),
        None,
    )
    .unwrap();
    // 项目在用 → 拒绝
    let proj = tempfile::tempdir().unwrap();
    register_usage(&paths, proj.path(), "dead", None);
    assert!(matches!(remove(&root(&install), "dead"),
        Err(SkillError::Referenced { key, .. }) if key == "dead"));
    // 登记簿清掉引用后删除成功：tag 清空、提交历史仍在
    hub::save_registry(
        &paths,
        &hub::ProjectRegistry::default(),
        &crate::lock::write_lock(&paths).unwrap(),
    )
    .unwrap();
    remove(&root(&install), "dead").unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(git::tag_list(&repo, "dead").unwrap().is_empty());
    assert!(repo.head().is_ok(), "hub 仓库历史应完好");
    // 再删同名 → 不存在
    assert!(matches!(
        remove(&root(&install), "dead"),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn restore_recovers_workdir_without_new_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "back", None, "复活");
    add(&root(&install), &src, None).unwrap();
    // 编辑面（工作副本）改出 v2 内容后发版，归档，再恢复
    fs::write(
        paths.skill_workdir("back").join("SKILL.md"),
        "---\nname: back\ndescription: 复活 v2\nversion: \"1.0.0\"\n---\n\nv2 正文\n",
    )
    .unwrap();
    release(&root(&install), "back").unwrap();
    archive(&root(&install), "back").unwrap();
    restore(&root(&install), "back").unwrap();
    // 工作副本按最新 tag 内容复活：frontmatter 全字段原样回来
    let text = fs::read_to_string(paths.skill_workdir("back").join("SKILL.md")).unwrap();
    assert!(text.contains("v2 正文"));
    let fm = skill_identity(&text, "SKILL.md").unwrap();
    assert_eq!(fm.name, "back");
    assert_eq!(fm.description, "复活 v2");
    assert_eq!(fm.version.as_deref(), Some("1.0.0"));
    // 版本链原样（无新 tag）、工作区干净
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "back").unwrap(),
        vec!["1.0.0", "0.1.0"]
    );
    assert!(!git::is_dirty(&repo).unwrap());
}

#[test]
fn restore_rejects_active_or_unknown() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "live", None, "活跃"),
        None,
    )
    .unwrap();
    assert!(matches!(
        restore(&root(&install), "live"),
        Err(SkillError::AlreadyExists { .. })
    ));
    assert!(matches!(
        restore(&root(&install), "ghost"),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn list_shows_rows_with_archive_flag_and_tag_filter() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "alpha", None, "甲"),
        None,
    )
    .unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "beta", None, "乙"),
        None,
    )
    .unwrap();
    archive(&root(&install), "beta").unwrap();
    // 给 alpha 补 source/tags：来源与标签只有 frontmatter 一处，改文件即可
    fs::write(
        paths.skill_workdir("alpha").join("SKILL.md"),
        "---\nname: alpha\ndescription: 甲\nversion: \"0.1.0\"\nsource: somewhere\ntags: [rust]\n---\n\n# alpha\n正文\n",
    )
    .unwrap();

    let rows = list(&root(&install), None).unwrap();
    let alpha = rows.iter().find(|r| r.key == "alpha").unwrap();
    let beta = rows.iter().find(|r| r.key == "beta").unwrap();
    assert_eq!(alpha.latest_version.as_deref(), Some("0.1.0"));
    assert!(!alpha.archived);
    assert_eq!(alpha.source.as_deref(), Some("somewhere"));
    assert!(beta.archived, "归档后 list 应标 archived");
    assert_eq!(beta.source, None, "没写 source 的行就是 None");
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
    )
    .unwrap();
    // 登记簿记一条引用（用法的唯一记录）
    let proj = tempfile::tempdir().unwrap();
    register_usage(&paths, proj.path(), "full", Some("0.1.0"));

    let d = show(&root(&install), "full").unwrap();
    assert_eq!(d.latest_version.as_deref(), Some("0.1.0"));
    assert!(!d.archived);
    assert_eq!(d.versions, vec!["0.1.0"]);
    let fm = d.frontmatter.as_ref().expect("活跃态应读到 frontmatter");
    assert_eq!(fm.description, "全量详情");
    assert_eq!(fm.version.as_deref(), Some("0.1.0"));
    assert_eq!(d.referenced_by.len(), 1);
    // 解引用后才可归档；归档后仍可 show（frontmatter 读自最新 tag 树）
    hub::save_registry(
        &paths,
        &hub::ProjectRegistry::default(),
        &crate::lock::write_lock(&paths).unwrap(),
    )
    .unwrap();
    archive(&root(&install), "full").unwrap();
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
fn diff_reports_per_file_unified_patch() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "delta", None, "差异");
    add(&root(&install), &src, None).unwrap();
    // 改 SKILL.md + 新增文件，发 0.2.0
    fs::write(
        paths.skill_workdir("delta").join("SKILL.md"),
        "---\nname: delta\ndescription: 差异\nversion: \"0.2.0\"\n---\n\n新正文\n",
    )
    .unwrap();
    fs::write(paths.skill_workdir("delta").join("NEW.md"), "新文件").unwrap();
    release(&root(&install), "delta").unwrap();

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
fn dry_runs_plan_actions_without_touching_disk() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "peek", None, "预演");
    // add 预演：动作清单齐全，store 一字未动
    let actions = add_dry(&root(&install), &src, None).unwrap();
    assert!(actions.iter().any(|a| a.contains("tag peek/v0.1.0")));
    // 缺省号写回也要出现在预演里，与真 add 的动作面对齐
    assert!(
        actions
            .iter()
            .any(|a| a == "写回 frontmatter version = 0.1.0")
    );
    assert!(paths.store().read_dir().unwrap().next().is_none());

    add(&root(&install), &src, None).unwrap();
    // release 预演：点名目标号，不产生新 tag
    set_version(&paths, "peek", "0.2.0");
    let actions = release_dry(&root(&install), "peek").unwrap();
    assert!(actions.iter().any(|a| a == "commit: release peek v0.2.0"));
    assert!(actions.iter().any(|a| a == "tag peek/v0.2.0"));
    assert_eq!(
        git::tag_list(&git2::Repository::open(paths.root()).unwrap(), "peek").unwrap(),
        vec!["0.1.0"]
    );

    // archive 预演：列出将删的编辑面与导出目录，盘上原封不动
    let actions = archive_dry(&root(&install), "peek").unwrap();
    assert!(actions.iter().any(|a| a == "remove store/peek"));
    assert!(actions.iter().any(|a| a == "remove export/peek/*"));
    assert!(paths.skill_workdir("peek").is_dir());

    // remove 预演：列出将删的 tag，盘上工作副本与 tag 都还在
    let actions = remove_dry(&root(&install), "peek").unwrap();
    assert!(actions.iter().any(|a| a == "remove store/peek"));
    assert!(actions.iter().any(|a| a == "delete tag peek/v0.1.0"));
    assert!(paths.skill_workdir("peek").is_dir());

    archive(&root(&install), "peek").unwrap();
    // restore 预演：点名最新 tag，工作副本仍未复活
    let actions = restore_dry(&root(&install), "peek").unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a == "checkout store/peek @ peek/v0.1.0")
    );
    assert!(!paths.skill_workdir("peek").is_dir());
}

#[test]
fn add_with_prefix_creates_nested_key() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "pdf", None, "嵌套键");
    let report = add(&root(&install), &src, Some("anthropics/skills")).unwrap();
    assert_eq!(report.key, "anthropics/skills/pdf");
    // 工作副本与 tag 全部按嵌套键就位
    assert!(
        paths
            .skill_workdir("anthropics/skills/pdf")
            .join("SKILL.md")
            .is_file()
    );
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "anthropics/skills/pdf").unwrap(),
        vec!["0.1.0"]
    );
    assert!(!git::is_dirty(&repo).unwrap());
    // 忽略规则深度无关：导出目录一律 ignored（无论键多深），工作副本始终 tracked
    assert!(
        repo.status_should_ignore(Path::new("export/anthropics/skills/pdf/0.1.0/SKILL.md"))
            .unwrap(),
        "多级键的导出路径应被忽略"
    );
    assert!(
        !repo
            .status_should_ignore(Path::new("store/anthropics/skills/pdf/SKILL.md"))
            .unwrap(),
        "工作副本应 tracked"
    );
}

#[test]
fn same_name_skills_coexist_under_different_keys() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let nested = make_skill(src_dir.path(), "pdf", None, "外来的");
    add(&root(&install), &nested, Some("anthropics/skills")).unwrap();
    let own = make_skill(src_dir.path(), "pdf", None, "自产的");
    add(&root(&install), &own, None).unwrap();

    // 两行都在，各自独立
    let rows = list(&root(&install), None).unwrap();
    assert!(rows.iter().any(|r| r.key == "anthropics/skills/pdf"));
    assert!(rows.iter().any(|r| r.key == "pdf"));

    // 各自可发版、互不干扰
    set_version(&paths, "pdf", "0.1.1");
    release(&root(&install), "pdf").unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "pdf").unwrap(), vec!["0.1.1", "0.1.0"]);
    assert_eq!(
        git::tag_list(&repo, "anthropics/skills/pdf").unwrap(),
        vec!["0.1.0"]
    );

    // 归档一个，另一个原封不动
    archive(&root(&install), "pdf").unwrap();
    assert!(paths.skill_workdir("anthropics/skills/pdf").is_dir());
    assert_eq!(
        git::tag_list(&repo, "anthropics/skills/pdf").unwrap(),
        vec!["0.1.0"]
    );
}

#[test]
fn remove_of_nested_key_spares_sibling_namespace() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let b = make_skill(src_dir.path(), "b-skill", None, "兄弟甲");
    add(&root(&install), &b, Some("a")).unwrap();
    let c = make_skill(src_dir.path(), "c-skill", None, "兄弟乙");
    add(&root(&install), &c, Some("a")).unwrap();

    remove(&root(&install), "a/b-skill").unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(git::tag_list(&repo, "a/b-skill").unwrap().is_empty());
    assert_eq!(
        git::tag_list(&repo, "a/c-skill").unwrap(),
        vec!["0.1.0"],
        "删除不得误删嵌套兄弟键的 tag"
    );
}

#[test]
fn add_rejects_name_directory_mismatch() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let dir = src_dir.path().join("dir-x");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: other\ndescription: 名不对\n---\n",
    )
    .unwrap();
    assert!(matches!(
        add(&root(&install), &dir, None),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn add_rejects_malformed_prefix() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "ok", None, "本身合法");
    // 纯空白前缀经规范化后等价空串，按无前缀处理；其余非法形态拒绝
    for bad in [
        "bad\\prefix",
        "a//b",
        "/lead",
        "tail/",
        "..",
        "a/./b",
        "C:/x",
        "1.2.3",
    ] {
        assert!(
            matches!(
                add(&root(&install), &src, Some(bad)),
                Err(SkillError::InvalidSource { .. })
            ),
            "非法前缀应被拒绝: {bad}"
        );
    }
    // 空串等价于无前缀：合法
    assert!(add(&root(&install), &src, Some("")).is_ok());
}

#[test]
fn add_rejects_key_inside_existing_skill_content() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let parent = make_skill(src_dir.path(), "pdf", None, "父键");
    add(&root(&install), &parent, None).unwrap();
    // 父键已是 skill：嵌套其下会被拒（两棵版本树会重叠）
    let nested = src_dir.path().join("inner");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join("SKILL.md"),
        "---\nname: inner\ndescription: 嵌套\n---\n",
    )
    .unwrap();
    assert!(
        matches!(
            add(&root(&install), &nested, Some("pdf")),
            Err(SkillError::InvalidSource { .. })
        ),
        "键落在已有 skill 内容树里应被拒绝"
    );
}

#[test]
fn add_rejects_reusing_existing_prefix_layer() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let nested = make_skill(src_dir.path(), "pdf", None, "先入嵌套键");
    add(&root(&install), &nested, Some("anthropics/skills")).unwrap();
    // 前缀层目录已被嵌套键占用：同名的扁平键不得再落进来
    let flat = src_dir.path().join("skills");
    fs::create_dir_all(&flat).unwrap();
    fs::write(
        flat.join("SKILL.md"),
        "---\nname: skills\ndescription: 撞前缀层\n---\n",
    )
    .unwrap();
    assert!(
        matches!(
            add(&root(&install), &flat, Some("anthropics")),
            Err(SkillError::AlreadyExists { .. })
        ),
        "键目录已被前缀层占用应被拒绝"
    );
}

#[test]
fn add_rejects_version_like_name() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let dir = src_dir.path().join("1.2.3");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: 1.2.3\ndescription: 形如版本目录\n---\n",
    )
    .unwrap();
    assert!(
        matches!(
            add(&root(&install), &dir, None),
            Err(SkillError::InvalidSource { .. })
        ),
        "目录名形如版本目录应被拒绝"
    );
}

#[test]
fn frontmatter_reads_five_fields_and_equivalent_tag_syntax() {
    let flow = "---\nname: pdf\ndescription: 处理 PDF\nversion: \"1.2.0\"\nsource: anthropics/skills\ntags: [pdf, office]\nlicense: MIT\n---\n\n正文\n";
    let block = "---\nname: pdf\ndescription: 处理 PDF\nversion: \"1.2.0\"\nsource: anthropics/skills\ntags:\n  - pdf\n  - office\nlicense: MIT\n---\n\n正文\n";
    let a = skill_identity(flow, "SKILL.md").unwrap();
    let b = skill_identity(block, "SKILL.md").unwrap();
    assert_eq!(a, b, "tags 流式与块式应等价，透传字段不参与解析");
    // 块式列表项顶格书写也合法，同样算 tags 的内容
    let flush = "---\nname: pdf\ndescription: 处理 PDF\nversion: \"1.2.0\"\nsource: anthropics/skills\ntags:\n- pdf\n- office\nlicense: MIT\n---\n\n正文\n";
    assert_eq!(skill_identity(flush, "SKILL.md").unwrap(), a);
    assert_eq!(a.name, "pdf");
    assert_eq!(a.description, "处理 PDF");
    assert_eq!(a.version.as_deref(), Some("1.2.0"));
    assert_eq!(a.source.as_deref(), Some("anthropics/skills"));
    assert_eq!(a.tags, vec!["pdf", "office"]);
}

#[test]
fn frontmatter_rejects_missing_fields_and_unquoted_or_bad_version() {
    for text in [
        "---\nname: pdf\ndescription: 描述\nversion: 1.2\n---\n",
        "---\nname: pdf\ndescription: 描述\nversion: 1.2.0\n---\n",
        "---\nname: pdf\ndescription: 描述\nversion: \"1.2\"\n---\n",
        "---\nname: pdf\ndescription: 描述\nversion: \"v1.2.0\"\n---\n",
    ] {
        assert!(
            matches!(
                skill_identity(text, "SKILL.md"),
                Err(SkillError::InvalidSource { .. })
            ),
            "非法 version 应被拒绝: {text}"
        );
    }
    assert!(matches!(
        skill_identity("---\nname: pdf\n---\n", "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
    assert!(matches!(
        skill_identity("---\ndescription: 描述\n---\n", "SKILL.md"),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn add_requires_description_and_quoted_version_but_not_source() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let no_desc = src_dir.path().join("no-desc");
    fs::create_dir_all(&no_desc).unwrap();
    fs::write(no_desc.join("SKILL.md"), "---\nname: no-desc\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_desc, None),
        Err(SkillError::InvalidSource { .. })
    ));
    let bare = src_dir.path().join("bare");
    fs::create_dir_all(&bare).unwrap();
    fs::write(
        bare.join("SKILL.md"),
        "---\nname: bare\ndescription: 裸号\nversion: 1.2.0\n---\n",
    )
    .unwrap();
    assert!(matches!(
        add(&root(&install), &bare, None),
        Err(SkillError::InvalidSource { .. })
    ));
    // 自产 skill 不写 source 也能入库，缺省 version 取首版 0.1.0
    let own = make_skill(src_dir.path(), "own", None, "自产");
    assert_eq!(add(&root(&install), &own, None).unwrap().version, "0.1.0");
}

#[test]
fn passthrough_fields_survive_release() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("pass");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("SKILL.md"),
        "---\nname: pass\ndescription: 透传\nlicense: MIT\ncompatibility: claude-code\nallowed-tools: [Bash, Read]\n---\n\n正文\n",
    )
    .unwrap();
    add(&root(&install), &src, None).unwrap();
    // 只改号发新版：写回只碰 version 行，其余字段与正文透传
    set_version(&paths, "pass", "0.1.1");
    release(&root(&install), "pass").unwrap();
    let after = fs::read_to_string(paths.skill_workdir("pass").join("SKILL.md")).unwrap();
    assert!(
        after.contains("version: \"0.1.1\""),
        "版本号应留在编辑面: {after}"
    );
    for kept in [
        "license: MIT",
        "compatibility: claude-code",
        "allowed-tools: [Bash, Read]",
        "---\n\n正文",
    ] {
        assert!(after.contains(kept), "透传不丢: {kept}");
    }
    // 版本树里的 SKILL.md 与编辑面逐字节一致
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        skill_md_from_tag(&repo, "pass/v0.1.1", "pass")
            .unwrap()
            .unwrap(),
        after
    );
}

#[test]
fn version_from_frontmatter_must_advance() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "adv", None, "递增"),
        None,
    )
    .unwrap();
    let work = paths.skill_workdir("adv").join("SKILL.md");
    let repo = git2::Repository::open(paths.root()).unwrap();

    // 手工抹掉号 → 号由文件定，没得发
    fs::write(&work, "---\nname: adv\ndescription: 递增\n---\n").unwrap();
    assert!(matches!(
        release(&root(&install), "adv"),
        Err(SkillError::InvalidSource { .. })
    ));
    // 与最新 tag 齐平或倒退 → 拒绝并提示改大
    for stale in ["0.1.0", "0.0.9"] {
        fs::write(
            &work,
            format!("---\nname: adv\ndescription: 递增\nversion: \"{stale}\"\n---\n"),
        )
        .unwrap();
        assert!(matches!(
            release(&root(&install), "adv"),
            Err(SkillError::VersionNotAdvancing { .. })
        ));
    }
    // 文件号领先 → 发版成功，tag = 文件号
    fs::write(
        &work,
        "---\nname: adv\ndescription: 递增\nversion: \"1.0.0\"\n---\n",
    )
    .unwrap();
    assert_eq!(release(&root(&install), "adv").unwrap(), "1.0.0");
    assert_eq!(git::tag_list(&repo, "adv").unwrap(), vec!["1.0.0", "0.1.0"]);
}

#[test]
fn archived_skill_reads_frontmatter_from_tag_tree() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "arch", Some("1.2.0"), "归档也读得到");
    fs::write(
        src.join("SKILL.md"),
        "---\nname: arch\ndescription: 归档也读得到\nversion: \"1.2.0\"\nsource: owner/repo\ntags: [pdf, office]\n---\n\n# arch\n正文\n",
    )
    .unwrap();
    add(&root(&install), &src, None).unwrap();
    archive(&root(&install), "arch").unwrap();

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
    assert_eq!(fm.tags, vec!["pdf", "office"]);
    assert!(!paths.root().join("meta").exists());
}

#[test]
fn clone_carries_all_skill_info_without_external_ledger() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "clone", None, "跟库走");
    fs::write(
        src.join("SKILL.md"),
        "---\nname: clone\ndescription: 跟库走\nversion: \"0.3.0\"\nsource: owner/repo\ntags: [a, b]\nlicense: MIT\n---\n\n# clone\n正文\n",
    )
    .unwrap();
    add(&root(&install), &src, None).unwrap();

    // 克隆 hub 仓库：内容 + 元数据 + 版本历史全在 git 里
    let dest = tempfile::tempdir().unwrap();
    let clone_root = dest.path().join("data");
    git2::Repository::clone(paths.root().to_str().unwrap(), &clone_root).unwrap();

    // 查询结果与原库逐字段一致（没有任何库外账本参与）
    assert_eq!(
        show(&clone_root, "clone").unwrap(),
        show(paths.root(), "clone").unwrap()
    );
    assert_eq!(
        list(&clone_root, Some("a")).unwrap(),
        list(paths.root(), Some("a")).unwrap()
    );
    assert!(!clone_root.join("meta").exists());
}
