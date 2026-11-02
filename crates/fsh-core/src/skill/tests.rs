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

    let report = add(&root(&install), &src, None).unwrap();
    assert_eq!(
        report,
        AddReport {
            key: "demo".into(),
            version: "0.1.0".into()
        }
    );
    // 工作副本与 meta 落盘
    assert!(paths.skill_workdir("demo").join("SKILL.md").is_file());
    // 缺省的号要落回编辑面：号只有一个诞生地（文件）
    let work = fs::read_to_string(paths.skill_workdir("demo").join("SKILL.md")).unwrap();
    assert!(
        work.contains("version: \"0.1.0\""),
        "缺省首版号应写回: {work}"
    );
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
    remove(&root(&install), "dup", false).unwrap();
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
fn bump_collects_dirty_changes_into_new_version() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "grow", None, "成长"),
        None,
    )
    .unwrap();
    // 工作副本改脏（编辑面随便改是常态）
    fs::write(
        paths.skill_workdir("grow").join("SKILL.md"),
        "---\nname: grow\ndescription: 成长 v2\n---\n\n新版正文\n",
    )
    .unwrap();
    let next = version_bump(&root(&install), "grow", Some(BumpPart::Minor)).unwrap();
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
        None,
    )
    .unwrap();
    remove(&root(&install), "gone", false).unwrap();
    assert!(matches!(
        version_bump(&root(&install), "gone", Some(BumpPart::Patch)),
        Err(SkillError::Archived { .. })
    ));
    assert!(matches!(
        version_bump(&root(&install), "never", Some(BumpPart::Patch)),
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
        None,
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
            key: "arch".into(),
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
        lock.entries.iter().any(|e| e.key == "arch"),
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
        None,
    )
    .unwrap();
    // 项目在用 → purge 拒绝
    let proj = tempfile::tempdir().unwrap();
    let manifest = SkillsManifest {
        skills: vec![deploy::SkillReq {
            key: "dead".into(),
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
        Err(SkillError::Referenced { key, .. }) if key == "dead"));
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
    add(&root(&install), &src, None).unwrap();
    // 编辑面（工作副本）改出 v2 内容后发版，归档，再恢复
    fs::write(
        paths.skill_workdir("back").join("SKILL.md"),
        "---\nname: back\ndescription: 复活 v2\n---\n\nv2 正文\n",
    )
    .unwrap();
    version_bump(&root(&install), "back", Some(BumpPart::Major)).unwrap();
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
    let alpha = rows.iter().find(|r| r.key == "alpha").unwrap();
    let beta = rows.iter().find(|r| r.key == "beta").unwrap();
    assert_eq!(alpha.latest_version.as_deref(), Some("0.1.0"));
    assert!(!alpha.archived);
    assert!(beta.archived, "归档后 list 应标 archived");
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
    // 项目 apply 产生引用
    let proj = tempfile::tempdir().unwrap();
    let manifest = SkillsManifest {
        skills: vec![deploy::SkillReq {
            key: "full".into(),
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
    add(&root(&install), &src, None).unwrap();
    // 改 SKILL.md + 新增文件，打包 0.2.0
    fs::write(
        paths.skill_workdir("delta").join("SKILL.md"),
        "---\nname: delta\ndescription: 差异\n---\n\n新正文\n",
    )
    .unwrap();
    fs::write(paths.skill_workdir("delta").join("NEW.md"), "新文件").unwrap();
    version_bump(&root(&install), "delta", Some(BumpPart::Minor)).unwrap();

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
    assert!(
        actions
            .iter()
            .any(|a| a.contains("materialize cache/peek/0.1.0"))
    );
    assert!(paths.store().read_dir().unwrap().next().is_none());

    add(&root(&install), &src, None).unwrap();
    // bump 预演：报出 0.1.0 -> 0.2.0，不产生新 tag
    let actions = bump_dry(&root(&install), "peek", Some(BumpPart::Minor)).unwrap();
    assert!(actions.iter().any(|a| a.contains("0.1.0 -> 0.2.0")));
    assert!(
        actions
            .iter()
            .any(|a| a == "写回 frontmatter version = 0.2.0")
    );
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

#[test]
fn add_with_prefix_creates_nested_key() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let src = make_skill(src_dir.path(), "pdf", None, "嵌套键");
    let report = add(&root(&install), &src, Some("anthropics/skills")).unwrap();
    assert_eq!(report.key, "anthropics/skills/pdf");
    // 工作副本、meta、tag、物化全部按嵌套键就位
    assert!(
        paths
            .skill_workdir("anthropics/skills/pdf")
            .join("SKILL.md")
            .is_file()
    );
    assert!(meta_path(&paths, "anthropics/skills/pdf").is_file());
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        git::tag_list(&repo, "anthropics/skills/pdf").unwrap(),
        vec!["0.1.0"]
    );
    assert!(
        paths
            .materialized("anthropics/skills/pdf", "0.1.0")
            .join("SKILL.md")
            .is_file()
    );
    // 物化住 cache 下：工作区仍干净（cache 整体被忽略，与键深度无关）
    assert!(!git::is_dirty(&repo).unwrap());
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
    version_bump(&root(&install), "pdf", Some(BumpPart::Patch)).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "pdf").unwrap(), vec!["0.1.1", "0.1.0"]);
    assert_eq!(
        git::tag_list(&repo, "anthropics/skills/pdf").unwrap(),
        vec!["0.1.0"]
    );

    // 归档一个，另一个原封不动
    remove(&root(&install), "pdf", false).unwrap();
    assert!(paths.skill_workdir("anthropics/skills/pdf").is_dir());
    assert_eq!(
        git::tag_list(&repo, "anthropics/skills/pdf").unwrap(),
        vec!["0.1.0"]
    );
}

#[test]
fn purge_of_nested_key_spares_sibling_namespace() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let b = make_skill(src_dir.path(), "b-skill", None, "兄弟甲");
    add(&root(&install), &b, Some("a")).unwrap();
    let c = make_skill(src_dir.path(), "c-skill", None, "兄弟乙");
    add(&root(&install), &c, Some("a")).unwrap();

    remove(&root(&install), "a/b-skill", true).unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(git::tag_list(&repo, "a/b-skill").unwrap().is_empty());
    assert_eq!(
        git::tag_list(&repo, "a/c-skill").unwrap(),
        vec!["0.1.0"],
        "purge 不得误删嵌套兄弟键的 tag"
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
fn purge_keeps_other_keys_tags_in_shared_namespace() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    let parent = make_skill(src_dir.path(), "a", None, "父键");
    add(&root(&install), &parent, None).unwrap();
    // 手工造一个嵌套子键的 tag（模拟外部或历史 tag）：purge 不得连坐
    let repo = git2::Repository::open(paths.root()).unwrap();
    git::tag_version(&repo, "a/vendor", "9.9.9").unwrap();
    remove(&root(&install), "a", true).unwrap();
    assert!(
        git::tag_list(&repo, "a").unwrap().is_empty(),
        "purge 应清掉本键 tag"
    );
    assert_eq!(
        git::tag_list(&repo, "a/vendor").unwrap(),
        vec!["9.9.9"],
        "子键命名空间的 tag 不得被父键 purge 连坐"
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
fn passthrough_fields_survive_release_and_materialization() {
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

    // --bump 只写 version 行，其余字段与正文一个字节不改
    version_bump(&root(&install), "pass", Some(BumpPart::Patch)).unwrap();
    let after = fs::read_to_string(paths.skill_workdir("pass").join("SKILL.md")).unwrap();
    assert!(
        after.contains("version: \"0.1.1\""),
        "代算号要写回 frontmatter: {after}"
    );
    for kept in [
        "license: MIT",
        "compatibility: claude-code",
        "allowed-tools: [Bash, Read]",
        "---\n\n正文",
    ] {
        assert!(after.contains(kept), "透传不丢: {kept}");
    }
    // 版本树与物化副本拿到同一份内容
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(
        skill_md_from_tag(&repo, "pass/v0.1.1", "pass")
            .unwrap()
            .unwrap(),
        after
    );
    assert_eq!(
        fs::read_to_string(paths.materialized("pass", "0.1.1").join("SKILL.md")).unwrap(),
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
        version_bump(&root(&install), "adv", None),
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
            version_bump(&root(&install), "adv", None),
            Err(SkillError::VersionNotAdvancing { .. })
        ));
    }
    // 文件号领先 → 发版成功，tag = 文件号
    fs::write(
        &work,
        "---\nname: adv\ndescription: 递增\nversion: \"1.0.0\"\n---\n",
    )
    .unwrap();
    assert_eq!(version_bump(&root(&install), "adv", None).unwrap(), "1.0.0");
    assert_eq!(git::tag_list(&repo, "adv").unwrap(), vec!["1.0.0", "0.1.0"]);
}

#[test]
fn bump_writes_version_back_into_frontmatter_before_release() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "auto", None, "代算"),
        None,
    )
    .unwrap();
    let work = paths.skill_workdir("auto").join("SKILL.md");
    // 入库已把缺省号 0.1.0 写进文件，代算在此号（=最新 tag）之上出新号
    assert!(
        fs::read_to_string(&work)
            .unwrap()
            .contains("version: \"0.1.0\"")
    );

    assert_eq!(
        version_bump(&root(&install), "auto", Some(BumpPart::Major)).unwrap(),
        "1.0.0"
    );
    // 写回发生在提交之前：编辑面与版本树里的 SKILL.md 都带新号，工作区随提交转干净
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert!(
        skill_md_from_tag(&repo, "auto/v1.0.0", "auto")
            .unwrap()
            .unwrap()
            .contains("version: \"1.0.0\"")
    );
    assert!(!git::is_dirty(&repo).unwrap());
}
