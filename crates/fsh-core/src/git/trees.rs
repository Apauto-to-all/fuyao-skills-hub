//! 内容树读取面：子树导出、树差异与树内 blob 收集。

use std::path::Path;
use std::time::Instant;

use git2::{ObjectType, Repository, build::CheckoutBuilder};

use super::GitError;
use super::Oid;
use super::resolve;

/// 把 tag 内容树中 subpath 子树导出到 dest 目录：剥掉子树前缀、不写仓库索引。
/// 导出产物是工作副本的还原来源，可随时删除后从 tag 重建。
pub fn materialize(
    repo: &Repository,
    tag: &str,
    subpath: &str,
    dest: &Path,
) -> Result<(), GitError> {
    let started = Instant::now();
    let (_, tree_id) = resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    // 子树剥成检出根：前缀天然剥除
    let subtree = tree
        .get_path(Path::new(subpath))?
        .to_object(repo)?
        .peel_to_tree()?;
    std::fs::create_dir_all(dest)?;
    let mut cb = CheckoutBuilder::new();
    // 导出到独立目录：不写仓库索引，工作区不受影响
    cb.target_dir(dest).force().update_index(false);
    repo.checkout_tree(subtree.as_object(), Some(&mut cb))?;
    // 外部交互与耗时：导出会落盘整棵子树，路径进字段便于定位产物；失败由上层边界记
    tracing::debug!(
        op = %"checkout",
        tag = %tag,
        path = %dest.display(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(())
}

/// 两棵 tag 内容树的统一 diff，按 unified patch 文本回调每行。
/// 回调返回 false 提前终止
pub fn diff_tree_files(
    repo: &Repository,
    tag1: &str,
    tag2: &str,
    mut on_line: impl FnMut(&git2::DiffDelta<'_>, &git2::DiffLine<'_>) -> bool,
) -> Result<(), GitError> {
    let (_, t1) = resolve(repo, tag1)?;
    let (_, t2) = resolve(repo, tag2)?;
    let tree1 = repo.find_tree(t1)?;
    let tree2 = repo.find_tree(t2)?;
    let diff = repo.diff_tree_to_tree(Some(&tree1), Some(&tree2), None)?;
    diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
        on_line(&delta, &line)
    })?;
    Ok(())
}

/// 收集 tag 内容树里 subpath 子树的全部 (相对路径, blob id)，先序遍历。
/// 相对路径显式由父路径拼出，不依赖遍历回调给出的 dir 前缀（该前缀在
/// 部分实现下只覆盖第一层）
pub fn subtree_blobs(
    repo: &Repository,
    tag: &str,
    subpath: &str,
) -> Result<Vec<(String, Oid)>, GitError> {
    let (_, tree_id) = resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let subtree = tree.get_path(Path::new(subpath))?;
    let subtree = subtree.to_object(repo)?.peel_to_tree()?;
    let mut out = Vec::new();
    collect_tree_blobs(repo, &subtree, "", &mut out)?;
    Ok(out)
}

/// 递归收集树内的全部 blob：路径 = 父路径 + 条目名，子树继续下钻
fn collect_tree_blobs(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
    out: &mut Vec<(String, Oid)>,
) -> Result<(), GitError> {
    for entry in tree.iter() {
        let name = entry.name().unwrap_or_default();
        let rel = format!("{prefix}{name}");
        match entry.kind() {
            Some(ObjectType::Blob) => out.push((rel, entry.id())),
            Some(ObjectType::Tree) => {
                let child = repo.find_tree(entry.id())?;
                collect_tree_blobs(repo, &child, &format!("{rel}/"), out)?;
            }
            _ => {}
        }
    }
    Ok(())
}
