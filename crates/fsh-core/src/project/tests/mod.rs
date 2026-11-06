//! 项目面测试：按被操作分组，共享夹具住在本文件。
//! 各测试只经项目使用与撤回的公开操作、登记簿和磁盘落点观察行为。

use std::fs;
use std::path::Path;

use super::*;
use crate::hub;

/// 建一次性 hub（初始化完成）+ 一个已入库技能
pub(super) fn setup_with_skill(version: Option<&str>) -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let src = src_root.path().join("demo");
    fs::create_dir_all(src.join("references")).unwrap();
    let fm_version = version
        .map(|v| format!("version: \"{v}\"\n"))
        .unwrap_or_default();
    fs::write(
        src.join("SKILL.md"),
        format!("---\nname: demo\n{fm_version}description: 演示\n---\n\n正文\n"),
    )
    .unwrap();
    fs::write(src.join("references/a.txt"), "ref-a").unwrap();
    crate::skill::add(paths.root(), &src, None).unwrap();
    (install, paths)
}

/// 建一个有项目目录的一次性项目（项目根必须已存在；不自动删，路径留给测试用）
pub(super) fn temp_project() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.keep();
    fs::create_dir_all(&p).unwrap();
    p
}

/// 一次 use 的 agent 列表（通用位 + dsh 两个不同落点）
pub(super) fn two_agents() -> Vec<String> {
    vec!["universal".to_string(), "dsh".to_string()]
}

/// 登记簿里某项目的该技能记录
pub(super) fn record(paths: &HubPaths, project: &Path, key: &str) -> Option<hub::ProjectSkill> {
    let reg = hub::load_registry(paths).unwrap();
    reg.projects
        .iter()
        .find(|p| p.matches(project))
        .and_then(|p| p.skills.iter().find(|s| s.key == key).cloned())
}

mod dest;
mod list;
mod spec;
mod unuse;
mod upgrade;

/// `use` 是关键字，使用流程测试住在 use.rs
#[path = "use.rs"]
mod use_flow;
