//! git 模块：对 libgit2 的封装，承载 hub 仓库的全部 git 操作。
//! hub 的 git 是自动审计日志：main 单分支、无 tag，写命令落盘成功后由
//! auto_commit 产生机器提交。模块保持路径无关——收路径与仓库句柄，
//! 不含 hub 目录布局知识；hub 布局由上层调用方负责。
//! 例外只有两个 hub 入口：只读姿势 open_hub 与写姿势 open_hub_for_write，
//! 让「装配判定 + 开仓」这份前导各只存在一份；写姿势的判定先于取锁，
//! 没建过的 hub 根上零落盘副作用。
//! 并发控制：写函数（init / commit_all / auto_commit）由上层入口在持有
//! 进程级文件锁（lock 模块）后调用——写入口统一经 open_hub_for_write 拿到
//! 凭证，装配入口经 write::run_assembly 取锁后开工，本模块其余函数不做自己的加锁。

mod auto;
mod repo;

pub(crate) use auto::sync_message;
pub use auto::{auto_commit, sync_manual};
pub use repo::{commit_all, init, is_dirty};

use std::path::Path;

use thiserror::Error;

use crate::ErrorCode;
use crate::error::CommonError;
use crate::lock::{self, WriteToken};
use crate::paths::HubPaths;

/// git2 原生类型的再导出：模块外（feature 生产代码与测试）一律经本模块取用，
/// 不再直接依赖 git2
pub use git2::{Oid, Repository};

/// git 模块错误：聚合底层 git2 与 IO 错误，供上层结构化匹配
#[derive(Debug, Error)]
pub enum GitError {
    /// 文件系统操作失败（目录创建等）
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// libgit2 操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git2::Error),
    /// 目标目录已是一个 git 仓库，拒绝重复初始化
    #[error("目录已是 git 仓库: {0}")]
    AlreadyRepo(String),
    /// hub 未初始化或装配不完整（缺 .git / .gitignore / hub.toml）
    #[error("hub 未初始化或装配不完整: {0}")]
    HubNotInitialized(String),
}

impl ErrorCode for GitError {
    fn error_code(&self) -> &'static str {
        match self {
            GitError::Io(_) => "io",
            GitError::Git(_) => "git",
            GitError::AlreadyRepo(_) => "already_repo",
            GitError::HubNotInitialized(_) => "hub_not_initialized",
        }
    }
}

/// 按 hub 根开仓（只读姿势）；装配不完整（缺 .git / .gitignore / hub.toml）
/// 给出结构化错误。判据只查存在，内容完好性归 doctor——判据一旦要读内容
/// 就变得可失败，失败被吞成「未初始化」会让 init 与判据互相矛盾。
/// 预演与查询共用这里：不取写锁，别人正在写时照样能读
pub fn open_hub(hub_root: &Path) -> Result<Repository, GitError> {
    if !crate::paths::HubPaths::from_root(hub_root).is_initialized() {
        return Err(GitError::HubNotInitialized(hub_root.display().to_string()));
    }
    Repository::open(hub_root).map_err(GitError::from)
}

/// 写姿势入口：先装配判定、再取 hub 全局写锁、锁内复判并开仓，随带路径族。
/// 判定只查存在、不读内容——内容完好性归 doctor；判据一旦要读内容就变得
/// 可失败，失败被吞成「未初始化」会让 init 与判据互相矛盾。判定因此零盘上
/// 副作用：hub 没建过时报 hub_not_initialized，不凭空创建 hub 根或锁文件
/// （取锁会创建锁文件的父目录，故判定必须先于取锁）。
/// 锁内顺序不变量（见 lock::WriteToken）：先取锁 → 再读登记簿 → 最后写盘；
/// 锁内对装配的复判保证读写都在同一把锁下，判定与取锁之间不留可写窗口。
/// 失败只有两种：装配判定与开仓失败、写锁被占用——正是公共底层错误的
/// Git 与 Lock 变体，各领域枚举经 Common 透传即可，`?` 在调用点直用
pub fn open_hub_for_write(
    hub_root: &Path,
) -> Result<(WriteToken, Repository, HubPaths), CommonError> {
    let paths = HubPaths::from_root(hub_root);
    if !paths.is_initialized() {
        return Err(GitError::HubNotInitialized(hub_root.display().to_string()).into());
    }
    let token = lock::write_lock(&paths)?;
    let repo = open_hub(hub_root)?;
    // 命令开工前先封存手改现场：干净时静默跳过，脏时提一条手改快照，
    // 保证命令结束后的操作提交只含本命令自身产生的变更
    sync_manual(&repo);
    Ok((token, repo, paths))
}

#[cfg(test)]
mod tests;
