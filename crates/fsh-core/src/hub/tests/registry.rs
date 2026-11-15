//! 项目登记簿的读写与引用判定。

use std::path::Path;

use crate::hub::{ProjectRegistry, ProjectSkill, RecordRef, Scope, load_registry, save_registry};
use crate::identity::project_key;
use crate::link::LinkKind;
use crate::paths::HubPaths;
use crate::project::Anchor;

/// 一条最小技能记录
fn skill(key: &str) -> ProjectSkill {
    ProjectSkill {
        key: key.into(),
        agents: vec!["universal".into()],
        link: LinkKind::Junction,
    }
}

/// 一份两段各有一条记录的登记簿：全局在前，两个项目按路径序在后
fn two_side_registry() -> ProjectRegistry {
    let mut reg = ProjectRegistry::default();
    reg.record_use(&Anchor::Global, skill("demo"), 0);
    for (dir, key) in [
        ("P:/beta", "demo"),
        ("P:/beta", "beta-skill"),
        ("P:/alpha", "demo"),
    ] {
        reg.record_use(
            &Anchor::Project(Path::new(dir).to_path_buf()),
            skill(key),
            1,
        );
    }
    reg
}

/// 逐记录行走的身份轴心：项目记录带出的路径就是登记键原样，
/// 消费方拿去做文件操作不必再懂「键是路径」
#[test]
fn records_yields_project_key_as_path() {
    let reg = two_side_registry();
    let paths: Vec<&Path> = reg.records().filter_map(|r| r.project_dir()).collect();
    // 每条技能记录各占一位：beta 登了两条技能就出现两次
    assert_eq!(
        paths,
        [
            Path::new("P:/alpha"),
            Path::new("P:/beta"),
            Path::new("P:/beta")
        ]
    );
}

/// 行走边界：空登记簿、空全局段只有项目记录、只有全局记录三种形态都要走通
#[test]
fn records_walks_registry_shapes() {
    // 空登记簿：两段都空
    assert_eq!(ProjectRegistry::default().records().count(), 0);
    // 空全局段：只有项目记录
    let mut project_only = ProjectRegistry::default();
    project_only.record_use(
        &Anchor::Project(Path::new("P:/a").to_path_buf()),
        skill("demo"),
        1,
    );
    assert_eq!(project_only.records().count(), 1);
    // 只有全局记录
    let mut global_only = ProjectRegistry::default();
    global_only.record_use(&Anchor::Global, skill("demo"), 0);
    assert_eq!(global_only.records().count(), 1);
}

/// 行走序：全局段在前，项目段按登记键序在后；每个项目内的技能按登记顺序
#[test]
fn records_walks_global_first_then_projects_in_key_order() {
    let reg = two_side_registry();
    let records: Vec<RecordRef> = reg.records().collect();
    let keys: Vec<&str> = records.iter().map(|r| r.skill().key.as_str()).collect();
    // 全局一条在前；项目段按登记键序，每个项目内的技能按登记顺序
    assert_eq!(keys, ["demo", "demo", "demo", "beta-skill"]);
    assert!(records[0].project_dir().is_none(), "第一条是全局记录");
    assert_eq!(
        records[1].project_dir(),
        Some(Path::new("P:/alpha")),
        "项目段按登记键序"
    );
    assert_eq!(records[3].skill().agents, vec!["universal".to_string()]);
}

/// 死项目记录（目录已不存在）也照常行走：键原样带出，不因死活筛记录
#[test]
fn records_walks_dead_project_records_too() {
    let mut reg = ProjectRegistry::default();
    reg.record_use(
        &Anchor::Project(Path::new("Z:/definitely/not/here").to_path_buf()),
        skill("demo"),
        1,
    );
    let records: Vec<RecordRef> = reg.records().collect();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].project_dir(),
        Some(Path::new("Z:/definitely/not/here")),
        "死记录的键原样带出"
    );
}

/// 项目身份清单：只列有项目记录的目录，死记录在内，空登记簿为空
#[test]
fn project_dirs_lists_identity_of_project_records() {
    assert!(ProjectRegistry::default().project_dirs().next().is_none());
    let reg = two_side_registry();
    let dirs: Vec<&Path> = reg.project_dirs().collect();
    assert_eq!(dirs, [Path::new("P:/alpha"), Path::new("P:/beta")]);
}

#[test]
fn save_registry_accepts_held_token() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    // 凭证随取随用：持锁写登记簿成功，语句结束凭证即释放
    let reg = ProjectRegistry::default();
    save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
    assert_eq!(load_registry(&paths).unwrap(), ProjectRegistry::default());
}

#[test]
fn registry_roundtrip_and_upsert() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    // 缺失文件 = 空登记簿
    assert_eq!(load_registry(&paths).unwrap(), ProjectRegistry::default());

    let mut reg = ProjectRegistry::default();
    reg.record_use(
        &Anchor::Project(install.path().to_path_buf()),
        skill("demo"),
        100,
    );
    reg.record_use(
        &Anchor::Project(install.path().to_path_buf()),
        skill("demo"),
        200,
    ); // 同路径再登记只刷新时间戳
    save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
    let back = load_registry(&paths).unwrap();
    assert_eq!(back.project_dirs().count(), 1);
    assert_eq!(back.project(install.path()).unwrap().last_used, 200);
}

/// 写出的形状：项目身份就是表键，技能块 header 自带完整路径
#[test]
fn save_writes_project_path_as_table_key() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    let mut reg = ProjectRegistry::default();
    reg.record_use(
        &Anchor::Project(install.path().to_path_buf()),
        skill("demo"),
        100,
    );
    save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();

    let text = std::fs::read_to_string(paths.projects_toml()).unwrap();
    // 引号形态由 TOML 写出器决定（反斜杠键会用字面量串），这里只认「键在 header 里」
    let key = project_key(install.path());
    assert!(
        text.lines()
            .any(|line| line.starts_with("[projects.") && line.contains(&key)),
        "项目身份应写成表键: {text}"
    );
    assert!(
        text.lines().any(|line| line.starts_with("[[projects.")
            && line.ends_with(".skills]]")
            && line.contains(&key)),
        "技能块 header 要带完整路径: {text}"
    );
    assert!(
        !text.contains("[[projects]]"),
        "不该再有匿名的项目数组: {text}"
    );
    assert!(
        !text.contains("path ="),
        "身份已是表键，记录里不再写 path: {text}"
    );
}

/// 两个项目倒序登记：写盘按表键排序，同一项目的技能相邻
#[test]
fn save_sorts_projects_and_skills_stably() {
    let mut reg = ProjectRegistry::default();
    reg.record_use(
        &Anchor::Project(Path::new("P:/beta").to_path_buf()),
        skill("demo-b"),
        2,
    );
    reg.record_use(
        &Anchor::Project(Path::new("P:/alpha").to_path_buf()),
        skill("demo-a"),
        1,
    );

    let text = toml::to_string(&reg).unwrap();
    let alpha = text.find("P:/alpha").unwrap();
    let beta = text.find("P:/beta").unwrap();
    assert!(alpha < beta, "项目按表键排序: {text}");
    assert!(
        text.contains("[[projects.\"P:/alpha\".skills]]"),
        "技能块挂在项目表键下: {text}"
    );
    assert_eq!(
        text.matches("[[projects").count(),
        2,
        "两个项目各一块: {text}"
    );
}

/// 写两次得到同一份文件：顺序与内容都不随写入次数变化
#[test]
fn saving_twice_produces_identical_text() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    let mut reg = ProjectRegistry::default();
    reg.record_use(
        &Anchor::Project(Path::new("P:/beta").to_path_buf()),
        skill("demo-b"),
        3,
    );
    reg.record_use(
        &Anchor::Project(Path::new("P:/alpha").to_path_buf()),
        skill("demo-a"),
        1,
    );
    // 同一项目再登记一条：身份命中，不新增项目记录
    reg.record_use(
        &Anchor::Project(Path::new("P:/beta").to_path_buf()),
        skill("demo-b2"),
        4,
    );
    let token = crate::lock::write_lock(&paths).unwrap();
    save_registry(&paths, &reg, &token).unwrap();
    let first = std::fs::read_to_string(paths.projects_toml()).unwrap();

    let reloaded = load_registry(&paths).unwrap();
    save_registry(&paths, &reloaded, &token).unwrap();
    let second = std::fs::read_to_string(paths.projects_toml()).unwrap();
    assert_eq!(first, second, "两次写入应得到逐字节相同的文件");
}

/// 空登记簿写出空文件，不留空表头
#[test]
fn empty_registry_writes_empty_file() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    save_registry(
        &paths,
        &ProjectRegistry::default(),
        &crate::lock::write_lock(&paths).unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(paths.projects_toml()).unwrap(),
        "",
        "没有用法记录时文件应当是空的"
    );
}

/// 同一目录换个写法不该多记一条；存下的键要能直接做文件操作
#[cfg(windows)]
#[test]
fn upsert_dedupes_by_project_identity() {
    let install = tempfile::tempdir().unwrap();
    let mut reg = ProjectRegistry::default();
    reg.record_use(
        &Anchor::Project(install.path().to_path_buf()),
        skill("demo"),
        100,
    );
    // 加 Windows 扩展前缀再登记一次：身份相同，只刷新时间戳
    let extended = std::path::PathBuf::from(format!("\\\\?\\{}", install.path().display()));
    reg.record_use(&Anchor::Project(extended), skill("demo"), 200);
    assert_eq!(
        reg.project_dirs().count(),
        1,
        "同一项目的两种写法只该有一条记录"
    );
    let record = reg.project(install.path()).unwrap();
    assert_eq!(record.last_used, 200, "命中应刷新时间戳");
    let key_dir = reg.project_dirs().next().unwrap();
    assert!(
        key_dir.is_dir(),
        "身份带出的路径要能直接用于文件操作: {key_dir:?}"
    );
}

/// 引用判定只读登记簿：技能列表里出现键即算一条使用记录
#[test]
fn usages_reads_one_row_per_record() {
    let mut reg = ProjectRegistry::default();
    // P:/a 只有项目身份、没有技能记录：不产生任何用法行
    reg.upsert(Path::new("P:/a"), 1);
    reg.record_use(
        &Anchor::Project(Path::new("P:/b").to_path_buf()),
        skill("demo"),
        2,
    );
    reg.record_use(&Anchor::Global, skill("demo"), 0);

    let usages = reg.usages("demo");
    assert_eq!(usages.len(), 2, "全局一条、项目一条");
    assert_eq!(usages[0].scope, Scope::Global);
    assert_eq!(usages[0].project, None, "全局记录没有项目");
    assert_eq!(usages[1].scope, Scope::Project);
    assert_eq!(
        usages[1].project.as_deref(),
        Some("P:/b"),
        "只列真正引用的项目"
    );
    assert_eq!(usages[1].agents, vec!["universal".to_string()]);
    assert!(reg.usages("other").is_empty());
}
