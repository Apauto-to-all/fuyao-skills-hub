//! skill 面测试：按被操作分组，共享夹具住在本文件。
//! 各测试只经 skill 模块的公开操作与登记簿观察行为。

use std::fs;
use std::path::{Path, PathBuf};

use super::*;

pub(super) fn setup() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    (install, paths)
}

/// 某键的版本号清单（新在前）：版本条目按 git::tag_entries 投影
pub(super) fn version_tags(repo: &git2::Repository, key: &str) -> Vec<String> {
    crate::git::tag_entries(repo, key)
        .unwrap()
        .into_iter()
        .map(|e| e.version)
        .collect()
}

/// hub 根路径（临时目录本身即 hub 根）
pub(super) fn root(install: &tempfile::TempDir) -> PathBuf {
    install.path().to_path_buf()
}

/// 造一个源 skill 目录（带 frontmatter）
pub(super) fn make_skill(dir: &Path, name: &str, version: Option<&str>, desc: &str) -> PathBuf {
    let skill_dir = dir.join(name);
    fs::create_dir_all(skill_dir.join("references")).unwrap();
    // 号是可选输入：给了就写进 metadata 子块
    let fm_version = version
        .map(|v| format!("metadata:\n  version: \"{v}\"\n"))
        .unwrap_or_default();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {desc}\n{fm_version}---\n\n# {name}\n正文\n"),
    )
    .unwrap();
    fs::write(skill_dir.join("references").join("a.txt"), "ref-a").unwrap();
    skill_dir
}

/// 改工作副本 frontmatter 里的号（发版输入是文件，fsh 只读不代算）
pub(super) fn set_version(paths: &HubPaths, key: &str, version: &str) {
    let file = paths.skill_workdir(key).join("SKILL.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, set_metadata_version(&text, version).unwrap()).unwrap();
}

/// 在登记簿里记一条项目引用（use 之后的状态）
pub(super) fn register_usage(paths: &HubPaths, project: &Path, key: &str, version: Option<&str>) {
    let mut reg = hub::load_registry(paths).unwrap();
    // 落点解析只认 agent 名单里的键：用内置的 universal，不依赖清单顺序
    reg.upsert(project, 1);
    reg.project_mut(project)
        .unwrap()
        .skills
        .push(hub::ProjectSkill {
            key: key.into(),
            version: version.map(str::to_string),
            agents: vec!["universal".to_string()],
            link: crate::link::LinkKind::Junction,
        });
    hub::save_registry(paths, &reg, &crate::lock::write_lock(paths).unwrap()).unwrap();
}

/// 在登记簿全局段记一条用法（use --global 之后的状态）
pub(super) fn register_global_usage(paths: &HubPaths, key: &str, agents: &[&str]) {
    let mut reg = hub::load_registry(paths).unwrap();
    reg.global_mut().skills.push(hub::ProjectSkill {
        key: key.into(),
        version: None,
        agents: agents.iter().map(|a| a.to_string()).collect(),
        link: crate::link::LinkKind::Junction,
    });
    hub::save_registry(paths, &reg, &crate::lock::write_lock(paths).unwrap()).unwrap();
}

mod add;
mod add_reject;
mod archive;
mod dry;
mod frontmatter;
mod key_shape;
mod misc;
mod query;
mod release;
mod remove;
mod restore;
mod state;
