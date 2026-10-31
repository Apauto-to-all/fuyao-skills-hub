use super::*;
use crate::deploy::{self, SkillsManifest};

/// 建一次性 hub（初始化完成、工作区干净）
fn setup() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    (install, paths)
}

/// hub 根路径（install\\data）
fn root(install: &tempfile::TempDir) -> PathBuf {
    install.path().join("data")
}

/// 造一个源 skill 目录（带 frontmatter）
fn make_skill(dir: &Path, name: &str, version: Option<&str>, desc: &str) -> PathBuf {
    let skill_dir = dir.join(name);
    fs::create_dir_all(skill_dir.join("references")).unwrap();
    let fm_version = version
        .map(|v| format!("version: {v}\n"))
        .unwrap_or_default();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\n{fm_version}description: {desc}\n---\n\n# {name}\n正文\n"),
    )
    .unwrap();
    fs::write(skill_dir.join("references").join("a.txt"), "ref-a").unwrap();
    skill_dir
}

#[test]
fn bump_semver_math() {
    assert_eq!(bump_semver("0.1.0", BumpPart::Patch).unwrap(), "0.1.1");
    assert_eq!(bump_semver("0.1.3", BumpPart::Minor).unwrap(), "0.2.0");
    assert_eq!(bump_semver("2.3.4", BumpPart::Major).unwrap(), "3.0.0");
    assert!(bump_semver("1.0", BumpPart::Patch).is_err());
    assert!(bump_semver("a.b.c", BumpPart::Patch).is_err());
}

#[test]
fn add_creates_workdir_meta_tag_and_materialized() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "demo", None, "示例技能");

    let report = add(&root(&install), &src).unwrap();
    assert_eq!(
        report,
        AddReport {
            skill: "demo".into(),
            version: "0.1.0".into()
        }
    );
    // 工作副本与 meta 落盘
    assert!(paths.skill_workdir("demo").join("SKILL.md").is_file());
    assert!(meta_path(&paths, "demo").is_file());
    // 首版 tag 与物化目录
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "demo").unwrap(), vec!["0.1.0"]);
    assert!(
        paths
            .materialized("demo", "0.1.0")
            .join("SKILL.md")
            .is_file()
    );
    // 提交完成、工作区干净
    assert!(!git::is_dirty(&repo).unwrap());
}

#[test]
fn add_honors_frontmatter_version() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "ver-skill", Some("2.1.0"), "带版本");
    let report = add(&root(&install), &src).unwrap();
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
        add(&root(&install), &empty),
        Err(SkillError::InvalidSource { .. })
    ));
    // SKILL.md 无 frontmatter
    let no_fm = src_dir.path().join("no-fm");
    fs::create_dir_all(&no_fm).unwrap();
    fs::write(no_fm.join("SKILL.md"), "# 只有正文\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_fm),
        Err(SkillError::InvalidSource { .. })
    ));
    // frontmatter 无 name
    let no_name = src_dir.path().join("no-name");
    fs::create_dir_all(&no_name).unwrap();
    fs::write(no_name.join("SKILL.md"), "---\ndescription: 无名\n---\n").unwrap();
    assert!(matches!(
        add(&root(&install), &no_name),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn add_rejects_duplicate_by_workdir_or_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "dup", None, "重复");
    add(&root(&install), &src).unwrap();
    // 工作副本占位 → 拒绝
    assert!(matches!(
        add(&root(&install), &src),
        Err(SkillError::AlreadyExists { .. })
    ));
    // 归档后（无工作副本、有 tag）→ 仍然拒绝
    remove(&root(&install), "dup", false).unwrap();
    assert!(!paths.skill_workdir("dup").exists());
    assert!(matches!(
        add(&root(&install), &src),
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
    add(&root(&install), &src).unwrap();
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
fn bump_collects_dirty_changes_into_new_version() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "grow", None, "成长"),
    )
    .unwrap();
    // 工作副本改脏（编辑面随便改是常态）
    fs::write(
        paths.skill_workdir("grow").join("SKILL.md"),
        "---\nname: grow\ndescription: 成长 v2\n---\n\n新版正文\n",
    )
    .unwrap();
    let next = version_bump(&root(&install), "grow", BumpPart::Minor).unwrap();
    assert_eq!(next, "0.2.0");
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "grow").unwrap(),
        vec!["0.2.0", "0.1.0"]
    );
    assert!(paths.materialized("grow", "0.2.0").is_dir());
    assert!(!git::is_dirty(&repo).unwrap(), "脏改动应被收编进新版本");
    // 新 tag 的 SKILL.md 内容 = 改后版本
    let text = skill_md_from_tag(&repo, "grow/v0.2.0", "grow")
        .unwrap()
        .unwrap();
    assert!(text.contains("新版正文"));
}

#[test]
fn bump_rejects_archived_or_missing() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "gone", None, "归档"),
    )
    .unwrap();
    remove(&root(&install), "gone", false).unwrap();
    assert!(matches!(
        version_bump(&root(&install), "gone", BumpPart::Patch),
        Err(SkillError::Archived { .. })
    ));
    assert!(matches!(
        version_bump(&root(&install), "never", BumpPart::Patch),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn archive_removes_workdir_but_keeps_versions_and_supply() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "arch", None, "归档供版"),
    )
    .unwrap();
    remove(&root(&install), "arch", false).unwrap();
    // 编辑面与 meta 删除
    assert!(!paths.skill_workdir("arch").exists());
    assert!(!meta_path(&paths, "arch").exists());
    // 版本链全在、历史提交可查
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "arch").unwrap(), vec!["0.1.0"]);
    assert!(!git::is_dirty(&repo).unwrap());
    // 归档不阻塞供版：项目声明它 → apply 照常铺出
    let proj = tempfile::tempdir().unwrap();
    let manifest = SkillsManifest {
        skills: vec![deploy::SkillReq {
            name: "arch".into(),
            version: "0.1.0".into(),
            target: "neutral".into(),
        }],
    };
    fs::write(
        HubPaths::project_manifest(proj.path()),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();
    deploy::apply(paths.root(), proj.path()).unwrap();
    let lock = deploy::read_lock(proj.path()).unwrap();
    assert!(
        lock.entries.iter().any(|e| e.name == "arch"),
        "归档 skill 仍应可 apply"
    );
}

#[test]
fn purge_removes_tags_and_rejects_when_referenced() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "dead", None, "待purge"),
    )
    .unwrap();
    // 项目在用 → purge 拒绝
    let proj = tempfile::tempdir().unwrap();
    let manifest = SkillsManifest {
        skills: vec![deploy::SkillReq {
            name: "dead".into(),
            version: "0.1.0".into(),
            target: "neutral".into(),
        }],
    };
    fs::write(
        HubPaths::project_manifest(proj.path()),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();
    deploy::apply(paths.root(), proj.path()).unwrap();
    assert!(matches!(remove(&root(&install), "dead", true),
        Err(SkillError::Referenced { skill, .. }) if skill == "dead"));
    // 解链后 purge 成功：tag 清空、提交历史仍在
    fs::remove_file(HubPaths::project_manifest(proj.path())).unwrap();
    deploy::apply(paths.root(), proj.path()).unwrap();
    remove(&root(&install), "dead", true).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(git::tag_list(&repo, "dead").unwrap().is_empty());
    assert!(git::resolve(&repo, "dead/stable").is_err());
    assert!(repo.head().is_ok(), "hub 仓库历史应完好");
    // 再 purge / remove 同名 → 不存在
    assert!(matches!(
        remove(&root(&install), "dead", true),
        Err(SkillError::SkillNotFound { .. })
    ));
}

#[test]
fn restore_recovers_workdir_without_new_tag() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "back", None, "复活");
    add(&root(&install), &src).unwrap();
    // 编辑面（工作副本）改出 v2 内容后发版，归档，再恢复
    fs::write(
        paths.skill_workdir("back").join("SKILL.md"),
        "---\nname: back\ndescription: 复活 v2\n---\n\nv2 正文\n",
    )
    .unwrap();
    version_bump(&root(&install), "back", BumpPart::Major).unwrap();
    remove(&root(&install), "back", false).unwrap();
    restore(&root(&install), "back").unwrap();
    // 工作副本按最新 tag 内容复活、meta 回来
    let text = fs::read_to_string(paths.skill_workdir("back").join("SKILL.md")).unwrap();
    assert!(text.contains("v2 正文"));
    assert!(meta_path(&paths, "back").is_file());
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
    )
    .unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "beta", None, "乙"),
    )
    .unwrap();
    remove(&root(&install), "beta", false).unwrap();
    // 给 alpha 补 tags 便于过滤（直接改 meta 再 add 无必要——手写 meta 测试数据）
    let meta = SkillMeta {
        source: "somewhere".into(),
        tags: vec!["rust".into()],
        author: None,
        license: None,
    };
    fs::write(meta_path(&paths, "alpha"), toml::to_string(&meta).unwrap()).unwrap();

    let rows = list(&root(&install), None).unwrap();
    let alpha = rows.iter().find(|r| r.name == "alpha").unwrap();
    let beta = rows.iter().find(|r| r.name == "beta").unwrap();
    assert_eq!(alpha.latest_version.as_deref(), Some("0.1.0"));
    assert!(!alpha.archived);
    assert!(beta.archived, "归档后 list 应标 archived");
    // tag 过滤
    let filtered = list(&root(&install), Some("rust")).unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "alpha");
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
    )
    .unwrap();
    // 项目 apply 产生引用
    let proj = tempfile::tempdir().unwrap();
    let manifest = SkillsManifest {
        skills: vec![deploy::SkillReq {
            name: "full".into(),
            version: "0.1.0".into(),
            target: "neutral".into(),
        }],
    };
    fs::write(
        HubPaths::project_manifest(proj.path()),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();
    deploy::apply(paths.root(), proj.path()).unwrap();

    let d = show(&root(&install), "full").unwrap();
    assert_eq!(d.latest_version.as_deref(), Some("0.1.0"));
    assert!(!d.archived);
    assert_eq!(d.versions, vec!["0.1.0"]);
    assert_eq!(d.description.as_deref(), Some("全量详情"));
    assert_eq!(d.referenced_by.len(), 1);
    // 归档后仍可 show（描述读自最新 tag）
    remove(&root(&install), "full", false).unwrap();
    let d2 = show(&root(&install), "full").unwrap();
    assert!(d2.archived);
    assert_eq!(d2.description.as_deref(), Some("全量详情"));
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
    add(&root(&install), &src).unwrap();
    // 改 SKILL.md + 新增文件，打包 0.2.0
    fs::write(
        paths.skill_workdir("delta").join("SKILL.md"),
        "---\nname: delta\ndescription: 差异\n---\n\n新正文\n",
    )
    .unwrap();
    fs::write(paths.skill_workdir("delta").join("NEW.md"), "新文件").unwrap();
    version_bump(&root(&install), "delta", BumpPart::Minor).unwrap();

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
    let actions = add_dry(&root(&install), &src).unwrap();
    assert!(actions.iter().any(|a| a.contains("tag peek/v0.1.0")));
    assert!(
        actions
            .iter()
            .any(|a| a.contains("materialize store/peek/0.1.0"))
    );
    assert!(paths.store().read_dir().unwrap().next().is_none());

    add(&root(&install), &src).unwrap();
    // bump 预演：报出 0.1.0 -> 0.2.0，不产生新 tag
    let actions = bump_dry(&root(&install), "peek", BumpPart::Minor).unwrap();
    assert!(actions.iter().any(|a| a.contains("0.1.0 -> 0.2.0")));
    assert_eq!(
        git::tag_list(&git2::Repository::open(paths.root()).unwrap(), "peek").unwrap(),
        vec!["0.1.0"]
    );

    // remove 预演（purge）：列出将删 tag，盘上工作副本与 tag 都还在
    let actions = remove_dry(&root(&install), "peek", true).unwrap();
    assert!(actions.iter().any(|a| a == "remove store/peek"));
    assert!(actions.iter().any(|a| a == "delete tag peek/v0.1.0"));
    assert!(paths.skill_workdir("peek").is_dir());

    remove(&root(&install), "peek", false).unwrap();
    // restore 预演：点名最新 tag，工作副本仍未复活
    let actions = restore_dry(&root(&install), "peek").unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a == "checkout store/peek @ peek/v0.1.0")
    );
    assert!(!paths.skill_workdir("peek").is_dir());
}
