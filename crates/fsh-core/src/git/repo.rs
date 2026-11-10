//! 仓库写路径：初始化、提交与版本 tag 的落盘操作。

use std::path::Path;
use std::time::Instant;

use git2::{Commit, IndexAddOption, Repository, RepositoryInitOptions, Signature};

use super::GitError;
use crate::git::Oid;

/// 在指定目录初始化 git 仓库：初始分支 main，仓库本地 core.autocrlf=false
/// （防止换行改写污染内容哈希）。目录已含 .git 时拒绝初始化。
pub fn init(dir: &Path) -> Result<Repository, GitError> {
    let started = Instant::now();
    // 目录下已有 .git 视为既有仓库，拒绝重复初始化以防误操作破坏历史
    if dir.join(".git").exists() {
        return Err(GitError::AlreadyRepo(dir.display().to_string()));
    }
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    let repo = Repository::init_opts(dir, &opts)?;
    // 关闭换行改写：工作副本字节与内容树保持一致，内容哈希才稳定
    repo.config()?.set_str("core.autocrlf", "false")?;
    // 外部交互与耗时：仓库初始化是写入口，记子命令名与耗时供排查；失败由上层命令边界记一次
    tracing::debug!(
        op = %"init",
        path = %dir.display(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(repo)
}

/// 把工作区全部变更（含未跟踪文件）提交为一个 commit，返回新 commit id。
/// 提交身份由调用方传入（来源 hub.yaml），本模块不读任何全局配置。
pub fn commit_all(
    repo: &Repository,
    message: &str,
    name: &str,
    email: &str,
) -> Result<Oid, GitError> {
    let mut index = repo.index()?;
    // 全量纳入工作区变更：新增与修改一次进索引
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;
    commit_tree(repo, index.write_tree()?, message, name, email)
}

/// 只提交指定路径（相对仓库根）：修复类操作不得把工作区其它未提交改动卷进提交。
/// 这些路径相对 HEAD 没有变化时不产生提交。
pub fn commit_paths(
    repo: &Repository,
    paths: &[&Path],
    message: &str,
    name: &str,
    email: &str,
) -> Result<(), GitError> {
    let mut index = repo.index()?;
    for path in paths {
        index.add_path(path)?;
    }
    index.write()?;
    let tree_id = index.write_tree()?;
    // 与 HEAD 同一棵树 = 这几个文件没有可提交的变化
    if let Ok(head) = repo.head()
        && let Some(target) = head.target()
        && repo.find_commit(target)?.tree_id() == tree_id
    {
        return Ok(());
    }
    commit_tree(repo, tree_id, message, name, email)?;
    Ok(())
}

/// 只提交指定子树（相对仓库根）：新增、修改与删除一并进索引，
/// 子树相对 HEAD 无变化时不产生提交。
/// 提交树由整个索引写出——子树之外已 `git add` 暂存的改动会随本次提交落地，
/// 未暂存的工作区改动不受影响。
pub fn commit_subtree(
    repo: &Repository,
    subtree: &Path,
    message: &str,
    name: &str,
    email: &str,
) -> Result<(), GitError> {
    let mut index = repo.index()?;
    // update_all 同步索引里的既有条目（工作区已删掉的文件从索引移除），
    // add_all 把新增与修改纳入索引
    index.update_all([subtree], None)?;
    index.add_all([subtree], IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_id = index.write_tree()?;
    // 与 HEAD 同一棵树 = 这棵子树没有可提交的变化
    if let Ok(head) = repo.head()
        && let Some(target) = head.target()
        && repo.find_commit(target)?.tree_id() == tree_id
    {
        return Ok(());
    }
    commit_tree(repo, tree_id, message, name, email)?;
    Ok(())
}

/// 用给定的树建一个 commit 并移动 HEAD，返回新 commit id
fn commit_tree(
    repo: &Repository,
    tree_id: Oid,
    message: &str,
    name: &str,
    email: &str,
) -> Result<Oid, GitError> {
    let started = Instant::now();
    let tree = repo.find_tree(tree_id)?;
    let sig = Signature::now(name, email)?;
    // 空仓库首次提交没有父提交
    let parent = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .map(|t| repo.find_commit(t))
        .transpose()?;
    let parents: Vec<&Commit<'_>> = parent.iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;
    // 外部交互与耗时：提交是写入口，只记子命令名与耗时，不记提交消息内容
    tracing::debug!(
        op = %"commit",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(oid)
}

/// 在 HEAD 上打版本 tag：`<skill>/v<semver>`。
pub fn tag_version(repo: &Repository, skill: &str, semver: &str) -> Result<Oid, GitError> {
    let started = Instant::now();
    let head = repo.head()?.peel_to_commit()?;
    let target = head.as_object();
    repo.tag_lightweight(&crate::naming::version_tag(skill, semver), target, true)?;
    // 外部交互与耗时：打 tag 是写入口，key / version 进字段，消息只说做了什么
    tracing::debug!(
        op = %"tag",
        key = %skill,
        version = %semver,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(head.id())
}

/// 工作区是否有未提交变更（显式包含未跟踪文件）。
pub fn is_dirty(repo: &Repository) -> Result<bool, GitError> {
    let mut opts = git2::StatusOptions::new();
    // 显式包含未跟踪文件，消除默认行为歧义
    opts.include_untracked(true);
    let dirty = !repo.statuses(Some(&mut opts))?.is_empty();
    Ok(dirty)
}

/// 删除一个版本 tag：`<skill>/v<semver>`。
pub fn tag_delete(repo: &Repository, tag: &str) -> Result<(), GitError> {
    repo.tag_delete(tag)?;
    Ok(())
}
