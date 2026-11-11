//! agent 配置测试的共享夹具。

use std::fs;

use super::*;
use crate::paths::HubPaths;

mod builtin;
mod global_root;
mod merge;

/// 建一个有 agents 目录的 hub 路径族
pub(super) fn setup() -> (tempfile::TempDir, HubPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(dir.path());
    fs::create_dir_all(paths.agents()).unwrap();
    (dir, paths)
}

/// 按 key 取一条，不存在即失败
pub(super) fn agent<'a>(agents: &'a [AgentInfo], key: &str) -> &'a AgentInfo {
    agents.iter().find(|a| a.key == key).unwrap()
}
