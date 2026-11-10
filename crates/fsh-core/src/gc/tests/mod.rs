//! gc 的端到端断言：真建 hub 与 git 仓库。夹具集中在这里，
//! 断言按被测概念分文件——导出目录清扫见 export.rs，登记簿死记录与 hub 装配见 records.rs。

use std::fs;

use super::*;
use crate::git as git_mod;
use crate::link;

/// 建一个带单版本技能的 hub：工作副本 + tag + 手工导出，无任何项目引用
fn hub_with_skill() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    let repo = git2::Repository::open(paths.root()).unwrap();
    let wd = paths.skill_workdir("demo");
    fs::create_dir_all(&wd).unwrap();
    fs::write(wd.join("SKILL.md"), "# demo\n").unwrap();
    git_mod::commit_all(&repo, "add demo", "fuyao", "fuyao@local").unwrap();
    git_mod::tag_version(&repo, "demo", "1.0.0").unwrap();
    git_mod::materialize(
        &repo,
        "demo/v1.0.0",
        "store/demo",
        &paths.exported("demo", "1.0.0"),
    )
    .unwrap();
    (install, paths)
}

/// 在登记簿里登记一个项目，并声明它引用了某技能某版本
fn registry_referencing(project: &Path, key: &str, version: Option<&str>) -> hub::ProjectRegistry {
    let mut reg = hub::ProjectRegistry::default();
    reg.upsert(project, 1);
    reg.project_mut(project)
        .unwrap()
        .skills
        .push(hub::ProjectSkill {
            key: key.into(),
            version: version.map(str::to_string),
            agents: vec!["agents".into()],
            link: link::LinkKind::Junction,
        });
    reg
}

mod export;
mod records;
