//! 自动提交：写命令成功落盘后的机器审计提交。
//! hub 的 git 是审计日志而非版本管理系统：脏检查先行，工作区干净时静默
//! 跳过（不产生空提交）；提交失败只告警——业务结果已落盘，绝不回滚。

use git2::Repository;

use super::GitError;
use super::repo::{commit_all, is_dirty};

/// 机器提交者名：固定身份写死在程序里，git log 里机器提交与人工提交一眼可分
const COMMITTER_NAME: &str = "fsh";
/// 机器提交者邮箱：与名配套的固定身份
const COMMITTER_EMAIL: &str = "fsh@local";

/// 写命令落盘成功后的自动提交：脏检查先行，无变更静默跳过；有变更则把
/// 全部改动提交为一个机器提交。提交失败只 WARN 告警并正常返回——
/// 审计日志失败不回滚业务，错误在这一层记一次，上层不再重复记
pub fn auto_commit(repo: &Repository, message: &str) {
    if let Err(e) = try_commit(repo, message) {
        tracing::warn!(cause = %e, "自动提交失败");
    }
}

/// 手改快照消息：写命令开工前封存现场用的固定文案
const SYNC_MESSAGE: &str = "sync(store): manual";

/// 写命令开工前的现场封存：仓库有未提交变更（用户手改）时先提一条
/// 手改快照，保证随后的操作提交只含本命令自身产生的变更；工作区
/// 干净时静默跳过。失败同样只告警——封存失败不阻塞业务
pub fn sync_manual(repo: &Repository) {
    if let Err(e) = try_commit(repo, SYNC_MESSAGE) {
        tracing::warn!(cause = %e, "手改快照提交失败");
    }
}

/// 一次自动提交的执行体：脏检查 + 全量提交
fn try_commit(repo: &Repository, message: &str) -> Result<(), GitError> {
    if !is_dirty(repo)? {
        return Ok(());
    }
    commit_all(repo, message, COMMITTER_NAME, COMMITTER_EMAIL)?;
    Ok(())
}
