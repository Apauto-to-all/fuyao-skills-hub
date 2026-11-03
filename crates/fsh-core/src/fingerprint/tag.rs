//! git tag 子树的内容指纹：先序遍历收集 (相对路径, blob id)，排序后喂
//! 「相对路径 + blob 字节」，与盘上侧同口径。

use std::path::Path;

use crate::git;

/// 以 store 子路径定位 tag 下的技能子树并计算指纹
pub(crate) fn tag_digest_at(repo: &git2::Repository, tag: &str, subpath: &str) -> Option<String> {
    hash_tag_contents(repo, tag, subpath).ok()
}

/// 计算 tag 子树的内容指纹；tag 或子树缺失时折叠为 `None`
fn hash_tag_contents(
    repo: &git2::Repository,
    tag: &str,
    subpath: &str,
) -> Result<String, git::GitError> {
    let (_, tree_id) = git::resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let subtree = tree.get_path(Path::new(subpath))?;
    let subtree = subtree.to_object(repo)?.peel_to_tree()?;

    // 逐树递归：子树的条目归属由父目录路径显式拼出，不依赖遍历回调给出的
    // dir 前缀（该前缀在部分实现下只覆盖第一层）
    let mut files: Vec<(String, git2::Oid)> = Vec::new();
    collect_tree_blobs(repo, &subtree, "", &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = blake3::Hasher::new();
    for (rel, oid) in &files {
        hasher.update(rel.as_bytes());
        hasher.update(repo.find_blob(*oid)?.content());
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// 递归收集树内的全部 blob：路径 = 父路径 + 条目名，子树继续下钻
fn collect_tree_blobs(
    repo: &git2::Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
    out: &mut Vec<(String, git2::Oid)>,
) -> Result<(), git::GitError> {
    for entry in tree.iter() {
        let name = entry.name().unwrap_or_default();
        let rel = format!("{prefix}{name}");
        match entry.kind() {
            Some(git2::ObjectType::Blob) => out.push((rel, entry.id())),
            Some(git2::ObjectType::Tree) => {
                let child = repo.find_tree(entry.id())?;
                collect_tree_blobs(repo, &child, &format!("{rel}/"), out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::dir::dir_digest;
    use crate::fingerprint::tests::build;

    /// 盘上目录与同内容 tag 子树的指纹相等（两把哈希同口径的证据）
    #[test]
    fn tag_digest_matches_disk_digest() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git::init(dir.path()).unwrap();
        let work = dir.path().join("store/demo");
        build(&work, &[("SKILL.md", "1"), ("references/y.md", "2")]);
        git::commit_all(&repo, "add demo", "t", "t@local").unwrap();
        git::tag_version(&repo, "demo", "1.0.0").unwrap();

        let disk = dir_digest(&work).unwrap();
        let tagged = tag_digest_at(&repo, "demo/v1.0.0", "store/demo").unwrap();
        assert_eq!(disk, tagged, "盘上目录与 tag 子树必须同口径");
    }

    /// tag 不存在时返回 None，而不是报错
    #[test]
    fn tag_digest_is_none_for_missing_tag() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git::init(dir.path()).unwrap();
        assert_eq!(tag_digest_at(&repo, "nope/v1.0.0", "store/nope"), None);
    }
}
