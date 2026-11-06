//! hub 目录模块的测试：按被测概念分文件，共享夹具集中在这里。

use std::fs;

use crate::git;
use crate::hub::{HubConfig, load_config};
use crate::paths::HubPaths;

mod config;
mod init;
mod registry;

/// 断言一条龙初始化后的完整落盘状态
pub(super) fn assert_initialized(paths: &HubPaths) {
    // 目录树齐全（元数据唯一住所在内容树里）
    for dir in [paths.store(), paths.export(), paths.agents()] {
        assert!(dir.is_dir(), "缺少目录: {dir:?}");
    }
    assert!(
        !paths.root().join("meta").exists(),
        "hub 布局里没有 meta 目录"
    );
    // .gitignore 是白名单：放行四样，其余一律忽略
    let gi = fs::read_to_string(paths.gitignore()).unwrap();
    assert_eq!(gi, "/*\n!/.gitignore\n!/hub.toml\n!/agents/\n!/store/\n");
    // hub.toml 可读回且是首版默认值
    let cfg = load_config(paths).unwrap().expect("hub.toml 应存在");
    assert_eq!(cfg, HubConfig::v1());
    // git 仓库存在、main 分支、关 autocrlf、工作区干净（首提交已完成）
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head = repo.find_reference("HEAD").unwrap();
    assert_eq!(
        head.symbolic_target().unwrap(),
        Some("refs/heads/main"),
        "初始分支应为 main"
    );
    assert_eq!(
        repo.config().unwrap().get_string("core.autocrlf").unwrap(),
        "false"
    );
    assert!(!git::is_dirty(&repo).unwrap(), "首提交后工作区应干净");
    // 全局写锁落在 hub 根下，且被 git 忽略：锁文件永远不进任何提交
    let lock = paths.root().join(".lock");
    assert!(lock.is_file(), "缺少全局写锁文件: {lock:?}");
    assert!(
        repo.status_should_ignore(std::path::Path::new(".lock"))
            .unwrap(),
        ".lock 必须被 git 忽略"
    );
    // 首提交身份来自 hub.toml
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.author().name().unwrap(), "fuyao-skills-hub");
    assert_eq!(commit.author().email().unwrap(), "fuyao-skills-hub@local");
}
