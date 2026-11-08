//! 项目登记簿的读写与引用判定。

use std::path::Path;

use crate::hub::registry::project_key;
use crate::hub::{ProjectRegistry, ProjectSkill, Scope, load_registry, save_registry};
use crate::link::LinkKind;
use crate::paths::HubPaths;

/// 一条最小技能记录
fn skill(key: &str) -> ProjectSkill {
    ProjectSkill {
        key: key.into(),
        version: None,
        agents: vec!["universal".into()],
        link: LinkKind::Junction,
    }
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
    reg.upsert(install.path(), 100);
    reg.upsert(install.path(), 200); // 同路径 upsert 只刷新时间戳
    save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
    let back = load_registry(&paths).unwrap();
    assert_eq!(back.projects.len(), 1);
    assert_eq!(back.project(install.path()).unwrap().last_used, 200);
}

/// 写出的形状：项目身份就是表键，技能块 header 自带完整路径
#[test]
fn save_writes_project_path_as_table_key() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    let mut reg = ProjectRegistry::default();
    reg.upsert(install.path(), 100);
    reg.project_mut(install.path())
        .unwrap()
        .skills
        .push(skill("demo"));
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
    reg.upsert(Path::new("P:/beta"), 2);
    reg.upsert(Path::new("P:/alpha"), 1);
    reg.project_mut(Path::new("P:/beta"))
        .unwrap()
        .skills
        .push(skill("demo-b"));
    reg.project_mut(Path::new("P:/alpha"))
        .unwrap()
        .skills
        .push(skill("demo-a"));

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
    reg.upsert(Path::new("P:/beta"), 2);
    reg.upsert(Path::new("P:/alpha"), 1);
    reg.upsert(Path::new("P:/beta"), 3); // 重复 upsert 只刷新时间戳
    for (path, key) in [("P:/beta", "demo-b"), ("P:/alpha", "demo-a")] {
        reg.project_mut(Path::new(path))
            .unwrap()
            .skills
            .push(skill(key));
    }
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
    reg.upsert(install.path(), 100);
    // 加 Windows 扩展前缀再登记一次：身份相同，只刷新时间戳
    let extended = std::path::PathBuf::from(format!("\\\\?\\{}", install.path().display()));
    reg.upsert(&extended, 200);
    assert_eq!(reg.projects.len(), 1, "同一项目的两种写法只该有一条记录");
    let (key, record) = reg.projects.iter().next().unwrap();
    assert_eq!(record.last_used, 200, "命中应刷新时间戳");
    assert!(Path::new(key).is_dir(), "存的键要能直接用于文件操作: {key}");
}

/// 引用判定只读登记簿：技能列表里出现键即算一条使用记录
#[test]
fn usages_reads_one_row_per_record() {
    let mut reg = ProjectRegistry::default();
    reg.upsert(Path::new("P:/a"), 1);
    reg.upsert(Path::new("P:/b"), 2);
    reg.project_mut(Path::new("P:/b"))
        .unwrap()
        .skills
        .push(skill("demo"));
    reg.global_mut().skills.push(skill("demo"));

    let usages = reg.usages("demo");
    assert_eq!(usages.len(), 2, "项目一条、全局一条");
    assert_eq!(usages[0].scope, Scope::Project);
    assert_eq!(
        usages[0].project.as_deref(),
        Some("P:/b"),
        "只列真正引用的项目"
    );
    assert_eq!(usages[0].agents, vec!["universal".to_string()]);
    assert_eq!(usages[0].version, None);
    assert_eq!(usages[1].scope, Scope::Global);
    assert_eq!(usages[1].project, None, "全局记录没有项目");
    assert!(reg.usages("other").is_empty());
}
