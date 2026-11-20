//! skill 面测试：按被操作分组，共享夹具住在本文件。
//! 各测试只经 skill 模块的公开操作与登记簿观察行为。

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::select::Selector;

pub(super) fn setup() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    (install, paths)
}

/// hub 根路径（临时目录本身即 hub 根）
pub(super) fn root(install: &tempfile::TempDir) -> PathBuf {
    install.path().to_path_buf()
}

/// 造一个源 skill 目录（带 frontmatter）
pub(super) fn make_skill(dir: &Path, name: &str, desc: &str) -> PathBuf {
    let skill_dir = dir.join(name);
    fs::create_dir_all(skill_dir.join("references")).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {desc}\n---\n\n# {name}\n正文\n"),
    )
    .unwrap();
    fs::write(skill_dir.join("references").join("a.txt"), "ref-a").unwrap();
    skill_dir
}

/// 归档一个键：按用户的写法把键拆成前缀 + 末段。批量语义只在 archive 那组用例里直接给选择器
pub(super) fn archive_key(
    hub: &Path,
    key: &str,
    dry_run: bool,
) -> Result<SelectionOutcome, SkillError> {
    let (prefix, name) = match key.rsplit_once('/') {
        Some((prefix, name)) => (Some(prefix.to_string()), name),
        None => (None, key),
    };
    archive(
        hub,
        &Selector::named(prefix, vec![name.to_string()]),
        dry_run,
    )
}

/// 恢复一个键：按用户的写法把键拆成前缀 + 末段。批量语义只在 restore 那组用例里直接给选择器
pub(super) fn restore_key(
    hub: &Path,
    key: &str,
    dry_run: bool,
) -> Result<SelectionOutcome, SkillError> {
    let (prefix, name) = match key.rsplit_once('/') {
        Some((prefix, name)) => (Some(prefix.to_string()), name),
        None => (None, key),
    };
    restore(
        hub,
        &Selector::named(prefix, vec![name.to_string()]),
        dry_run,
    )
}

/// 在登记簿里记一条项目引用（use 之后的状态）
pub(super) fn register_usage(paths: &HubPaths, project: &Path, key: &str) {
    let mut reg = hub::load_registry(paths).unwrap();
    // 落点解析只认 agent 名单里的键：用内置的 universal，不依赖清单顺序
    reg.record_use(
        &crate::project::Anchor::Project(project.to_path_buf()),
        hub::ProjectSkill {
            key: key.into(),
            agents: vec!["universal".to_string()],
            link: crate::link::LinkKind::Junction,
        },
        1,
    );
    hub::save_registry(paths, &reg, &crate::lock::write_lock(paths).unwrap()).unwrap();
}

/// 在登记簿全局段记一条用法（use --global 之后的状态）
pub(super) fn register_global_usage(paths: &HubPaths, key: &str, agents: &[&str]) {
    let mut reg = hub::load_registry(paths).unwrap();
    reg.record_use(
        &crate::project::Anchor::Global,
        hub::ProjectSkill {
            key: key.into(),
            agents: agents.iter().map(|a| a.to_string()).collect(),
            link: crate::link::LinkKind::Junction,
        },
        1,
    );
    hub::save_registry(paths, &reg, &crate::lock::write_lock(paths).unwrap()).unwrap();
}

mod add;
mod add_reject;
mod archive;
mod dry;
mod frontmatter;
mod key_shape;
mod list;
mod misc;
#[path = "move.rs"]
mod move_flow;
mod remove;
mod restore;
mod state;
