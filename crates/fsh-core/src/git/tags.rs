//! tag 命名空间的读取面：版本条目、按键遍历与 blob 读取。

use std::time::Instant;

use git2::{ObjectType, Repository};

use super::GitError;
use super::Oid;

/// 版本条目：版本号、发布提交短哈希与提交时间
#[derive(Debug, Clone, PartialEq)]
pub struct TagEntry {
    /// 版本号（tag 名的尾段）
    pub version: String,
    /// 发布提交的短哈希（7 位）
    pub commit: String,
    /// 发布提交的时间（Unix 秒）
    pub date: u64,
}

/// 短哈希的固定位数：提交 id 恒为 40 位十六进制，截前 7 位足够区分
const SHORT_HASH_LEN: usize = 7;

/// 列出某 skill 的全部版本条目（新版本在前）：版本号、发布提交短哈希与提交时间。
/// 归属判定走 naming 的尾段语义：嵌套子键（如 a/b 之下的 a/b/vendor）的
/// 引用不属于本键
pub fn tag_entries(repo: &Repository, skill: &str) -> Result<Vec<TagEntry>, GitError> {
    let started = Instant::now();
    let mut entries: Vec<TagEntry> = Vec::new();
    // shortcut: 只认轻量 tag（oid 直接是提交）；出现注解 tag 时改走 resolve 剥离
    repo.tag_foreach(|oid, name| {
        let full = String::from_utf8_lossy(name);
        let Some((key, version)) = crate::naming::parse_version_tag_ref(&full) else {
            return true;
        };
        if key != skill {
            return true;
        }
        if let Ok(commit) = repo.find_commit(oid) {
            entries.push(TagEntry {
                version: version.to_string(),
                commit: commit.id().to_string()[..SHORT_HASH_LEN].to_string(),
                date: commit.time().seconds().max(0).cast_unsigned(),
            });
        }
        true
    })?;
    // 按语义化版本号降序：新版本在前
    entries.sort_by_key(|e| std::cmp::Reverse(crate::version::Version::parse(&e.version)));
    // 查询命中数与耗时：遍历全部 tag 后才知道本键有几个版本，命中数是排查时间线的关键
    tracing::debug!(
        op = %"tag_entries",
        key = %skill,
        count = entries.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(entries)
}

/// 收集某键命名空间下的全部版本 tag 短名（含删 tag 预演、fingerprint 等全部
/// 「只要名字」的调用方）。归属判定与 tag_entries 同一尾段语义
pub fn tag_keys(repo: &Repository, key: &str) -> Result<Vec<String>, GitError> {
    let mut tags = Vec::new();
    repo.tag_foreach(|_oid, name| {
        let full = String::from_utf8_lossy(name);
        if crate::naming::key_from_tag_ref(&full) == Some(key) {
            tags.push(full.strip_prefix("refs/tags/").unwrap_or(&full).to_string());
        }
        true
    })?;
    Ok(tags)
}

/// 遍历全部版本 tag 反解出的 skill 唯一键（tag 命名空间的键全集）
pub fn tag_keys_all(repo: &Repository) -> Result<Vec<String>, GitError> {
    let mut keys: Vec<String> = Vec::new();
    repo.tag_foreach(|_oid, name| {
        if let Some(key) = crate::naming::key_from_tag_ref(&String::from_utf8_lossy(name))
            && !keys.iter().any(|e| e == key)
        {
            keys.push(key.to_string());
        }
        true
    })?;
    keys.sort();
    Ok(keys)
}

/// 解析 tag：返回其最终指向的 (commit id, 内容树 id)，调用方按需取用。
pub fn resolve(repo: &Repository, tag: &str) -> Result<(Oid, Oid), GitError> {
    // 解析失败（不存在 / 非 tag / 无法剥离到 commit）一律视为 tag 不存在
    let obj = repo
        .revparse_single(tag)
        .map_err(|_| GitError::TagNotFound {
            tag: tag.to_string(),
        })?;
    let commit = obj.peel_to_commit().map_err(|_| GitError::TagNotFound {
        tag: tag.to_string(),
    })?;
    let tree = commit.tree()?;
    Ok((commit.id(), tree.id()))
}

/// 读 tag 内容树里 subpath 指向的 blob 文本；子树里没有该路径时返回
/// None。归档态读 frontmatter 用
pub fn read_blob_at(
    repo: &Repository,
    tag: &str,
    subpath: &str,
) -> Result<Option<String>, GitError> {
    let (_, tree_id) = resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let entry = match tree.get_path(std::path::Path::new(subpath)) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let obj = entry.to_object(repo)?;
    if obj.kind() != Some(ObjectType::Blob) {
        return Ok(None);
    }
    let blob = obj.peel_to_blob()?;
    Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
}
