//! git tag 子树的内容指纹：先序遍历收集 (相对路径, blob id)，排序后喂
//! 「相对路径 + blob 字节」，与盘上侧同口径。

use crate::git;

/// 以 store 子路径定位 tag 下的技能子树并计算指纹
pub(crate) fn tag_digest_at(repo: &git::Repository, tag: &str, subpath: &str) -> Option<String> {
    hash_tag_contents(repo, tag, subpath).ok()
}

/// 计算 tag 子树的内容指纹；tag 或子树缺失时折叠为 `None`
fn hash_tag_contents(
    repo: &git::Repository,
    tag: &str,
    subpath: &str,
) -> Result<String, git::GitError> {
    // 子树 blob 收集与排序由 git 模块统一提供
    let mut files = git::subtree_blobs(repo, tag, subpath)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = blake3::Hasher::new();
    for (rel, oid) in &files {
        hasher.update(rel.as_bytes());
        hasher.update(repo.find_blob(*oid)?.content());
    }
    Ok(hasher.finalize().to_hex().to_string())
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
