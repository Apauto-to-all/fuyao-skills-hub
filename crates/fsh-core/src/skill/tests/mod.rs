//! skill 面测试：按被操作分组，共享夹具住在本文件。
//! 各测试只经 skill 模块的公开操作与登记簿观察行为。

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

pub(super) fn setup() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    (install, paths)
}

/// hub 根路径（install\data）
pub(super) fn root(install: &tempfile::TempDir) -> PathBuf {
    install.path().join("data")
}

/// 造一个源 skill 目录（带 frontmatter）
pub(super) fn make_skill(dir: &Path, name: &str, version: Option<&str>, desc: &str) -> PathBuf {
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
pub(super) fn set_version(paths: &HubPaths, key: &str, version: &str) {
    let file = paths.skill_workdir(key).join("SKILL.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, set_frontmatter_version(&text, version).unwrap()).unwrap();
}

/// 在登记簿里记一条项目引用（use 之后的状态）
pub(super) fn register_usage(paths: &HubPaths, project: &Path, key: &str, version: Option<&str>) {
    let mut reg = hub::load_registry(paths).unwrap();
    // 落点解析只认 agent 名单里的键：用内置的 universal，不依赖清单顺序
    reg.upsert(project, 1);
    reg.projects[0].skills.push(hub::ProjectSkill {
        key: key.into(),
        version: version.map(str::to_string),
        agents: vec!["universal".to_string()],
        link: crate::link::LinkKind::Junction,
    });
    hub::save_registry(paths, &reg, &crate::lock::write_lock(paths).unwrap()).unwrap();
}

mod add;
mod add_reject;
mod archive;
mod frontmatter;
mod misc;
mod query;
mod release;
mod remove;
mod restore;
