//! 盘上目录的内容指纹：递归收集相对路径（排序归一）后喂「相对路径 + 文件字节」。
//! 遍历序靠排序抵消，两侧同口径是 [`super::tag`] 能比对的前提。

use std::fs;
use std::path::Path;

/// 目录内容指纹；目录不存在或读不了时把 IO 错误原样上报
pub(crate) fn dir_digest(dir: &Path) -> std::io::Result<String> {
    let mut files = Vec::new();
    collect_files_sorted(dir, "", &mut files)?;
    let mut hasher = blake3::Hasher::new();
    for rel in &files {
        hasher.update(rel.as_bytes());
        hasher.update(&fs::read(dir.join(rel))?);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// 收集「相对路径 → 文件名」，按相对路径排序，抵消两侧遍历序差异
fn collect_files_sorted(dir: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let rel = if prefix.is_empty() {
            name.to_string_lossy().to_string()
        } else {
            format!("{prefix}/{}", name.to_string_lossy())
        };
        let is_dir = fs::metadata(entry.path())
            .map(|m| m.is_dir())
            .unwrap_or(false);
        if is_dir {
            collect_files_sorted(&entry.path(), &rel, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::tests::build;

    /// 同一份内容换个创建顺序，指纹必须相同
    #[test]
    fn digest_ignores_creation_order() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        build(a.path(), &[("x.txt", "1"), ("sub/y.txt", "2")]);
        build(b.path(), &[("sub/y.txt", "2"), ("x.txt", "1")]);
        assert_eq!(dir_digest(a.path()).unwrap(), dir_digest(b.path()).unwrap());
    }

    /// 内容不同则指纹不同
    #[test]
    fn digest_distinguishes_content() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        build(a.path(), &[("x.txt", "1")]);
        build(b.path(), &[("x.txt", "2")]);
        assert_ne!(dir_digest(a.path()).unwrap(), dir_digest(b.path()).unwrap());
    }

    /// 路径不同、字节相同，指纹必须不同（路径参与哈希）
    #[test]
    fn digest_includes_relative_path() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        build(a.path(), &[("x.txt", "same")]);
        build(b.path(), &[("y.txt", "same")]);
        assert_ne!(dir_digest(a.path()).unwrap(), dir_digest(b.path()).unwrap());
    }

    /// 空目录有确定值，且与「一个空文件」不同
    #[test]
    fn empty_dir_has_stable_digest() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_eq!(dir_digest(a.path()).unwrap(), dir_digest(b.path()).unwrap());

        let c = tempfile::tempdir().unwrap();
        build(c.path(), &[("x.txt", "")]);
        assert_ne!(dir_digest(a.path()).unwrap(), dir_digest(c.path()).unwrap());
    }

    /// 目录不存在时报 IO 错误，不静默返回一个值
    #[test]
    fn digest_reports_missing_dir() {
        let ok = tempfile::tempdir().unwrap();
        let gone = ok.path().join("no-such-subdir");
        assert!(dir_digest(&gone).is_err());
    }
}
