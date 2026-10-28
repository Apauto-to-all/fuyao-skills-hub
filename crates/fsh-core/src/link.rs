//! 目录链接引擎：把「源目录以链接形式出现在目标位置」抽象为一个原语。
//! 三级降级（Windows 实况）：目录符号链接 → NTFS junction → 带记账的拷贝。
//! 拷贝模式在目标侧写 .fsh-source.json（来源 + 内容哈希），使漂移检测可复现。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 链接错误
#[derive(Debug, Error)]
pub enum LinkError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// JSON 序列化失败
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    /// 目标位置已存在非本引擎管理的目录，拒绝覆盖
    #[error("目标已存在且不受管理: {0}")]
    Occupied(String),
    /// 撤销时目标不是链接也不是拷贝记账目录
    #[error("目标不是可撤销的链接: {0}")]
    NotALink(String),
}

/// 实际采用的链接方式（三级降级的结果）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkKind {
    /// 目录符号链接（需开发者模式或提权）
    Symlink,
    /// NTFS junction（无门槛，主力）
    Junction,
    /// 拷贝 + .fsh-source.json 记账
    Copy,
}

/// 拷贝模式的来源记账：漂移检测据此重算内容哈希比对
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceManifest {
    /// 源目录绝对路径
    pub source: PathBuf,
    /// 源内容树哈希（blake3）
    pub hash: String,
}

/// 拷贝记账文件名（位于拷贝目标目录内）
pub const SOURCE_MANIFEST_FILE: &str = ".fsh-source.json";

/// 读取拷贝目标里的来源记账；不存在返回 None（说明不是拷贝模式）
pub fn load_source_manifest(dir: &Path) -> Result<Option<SourceManifest>, LinkError> {
    let p = dir.join(SOURCE_MANIFEST_FILE);
    if !p.is_file() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(p)?)?))
}

/// 在 dst 处建立指向 src 的目录链接：按符号链接 → junction → 拷贝
/// 依次尝试，返回实际采用的方式。dst 已存在时拒绝。
pub fn create_dir_link(src: &Path, dst: &Path, hash: &str) -> Result<LinkKind, LinkError> {
    // 任何已存在的目标都拒绝：只在自己管理的空位上落链接
    if dst.symlink_metadata().is_ok() {
        return Err(LinkError::Occupied(dst.display().to_string()));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    // 一级：目录符号链接（Windows 需开发者模式或提权，失败即降级）
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(src, dst).is_ok() {
            return Ok(LinkKind::Symlink);
        }
    }
    // 二级：NTFS junction（无门槛主力）
    if junction::create(src, dst).is_ok() {
        return Ok(LinkKind::Junction);
    }
    // 三级：拷贝 + 记账
    copy_dir_recursive(src, dst)?;
    let manifest = SourceManifest {
        source: src.canonicalize().unwrap_or_else(|_| src.to_path_buf()),
        hash: hash.to_string(),
    };
    let p = dst.join(SOURCE_MANIFEST_FILE);
    fs::write(p, serde_json::to_string_pretty(&manifest)?)?;
    Ok(LinkKind::Copy)
}

/// 撤销链接：三种方式建立的都能撤（符号链接/junction 只删自身；
/// 拷贝删整目录含记账）。目标不存在视为已撤销（幂等）。
pub fn remove_link(dst: &Path) -> Result<(), LinkError> {
    let meta = match fs::symlink_metadata(dst) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if meta.file_type().is_symlink() {
        // 符号链接与 junction 都只删 reparse point 自身，不碰目标内容
        fs::remove_dir(dst)?;
        Ok(())
    } else if meta.is_dir() && load_source_manifest(dst)?.is_some() {
        // 拷贝模式：整目录（含记账文件）删除
        fs::remove_dir_all(dst)?;
        Ok(())
    } else {
        Err(LinkError::NotALink(dst.display().to_string()))
    }
}

/// 递归拷贝目录（标准库没有递归 copy，自己走一遍）
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个带两个文件的真实源目录
    fn make_src() -> tempfile::TempDir {
        let src = tempfile::tempdir().unwrap();
        fs::write(src.path().join("SKILL.md"), "# demo\n").unwrap();
        fs::create_dir(src.path().join("refs")).unwrap();
        fs::write(src.path().join("refs/a.md"), "a\n").unwrap();
        src
    }

    /// 断言「返回的方式」与「盘上事实」一致
    fn assert_on_disk(kind: LinkKind, dst: &Path) {
        let meta = fs::symlink_metadata(dst).unwrap();
        match kind {
            LinkKind::Symlink => {
                assert!(meta.file_type().is_symlink(), "应表现为符号链接");
            }
            LinkKind::Junction => {
                // junction 在 Rust 侧也表现为 symlink（reparse point）
                assert!(
                    meta.file_type().is_symlink(),
                    "junction 应表现为 reparse point"
                );
                // 且能穿透读到源内容
                assert!(dst.join("SKILL.md").is_file());
            }
            LinkKind::Copy => {
                assert!(meta.is_dir(), "应表现为真实目录");
                let m = load_source_manifest(dst).unwrap().expect("拷贝必有记账");
                assert!(m.source.is_absolute(), "记账应记源绝对路径");
                assert!(!m.hash.is_empty(), "记账应记内容哈希");
                assert!(dst.join(SOURCE_MANIFEST_FILE).is_file());
            }
        }
    }

    #[test]
    fn create_link_falls_back_and_is_readable() {
        let src = make_src();
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("linked");
        let kind = create_dir_link(src.path(), &dst, "abc123").unwrap();
        assert_on_disk(kind, &dst);
        // 无论哪一级，「读 dst 应能看到源文件」（拷贝模式含记账文件）
        assert!(dst.join("SKILL.md").is_file());
        assert!(dst.join("refs/a.md").is_file());
        // 撤销后干净（记账文件也一并消失）
        remove_link(&dst).unwrap();
        assert!(!dst.exists(), "撤销后目标应不存在");
        // 源不动
        assert!(src.path().join("SKILL.md").is_file());
    }

    #[test]
    fn create_link_refuses_occupied_dst() {
        let src = make_src();
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("occupied");
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("user-file.txt"), "x").unwrap();
        assert!(matches!(
            create_dir_link(src.path(), &dst, "h"),
            Err(LinkError::Occupied(_))
        ));
        // 用户文件不得被动
        assert!(dst.join("user-file.txt").is_file());
    }

    #[test]
    fn remove_link_is_idempotent_on_missing() {
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("never-existed");
        remove_link(&dst).unwrap();
    }

    #[test]
    fn remove_link_refuses_foreign_directory() {
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("foreign");
        fs::create_dir_all(&dst).unwrap();
        fs::write(dst.join("mine.txt"), "x").unwrap();
        assert!(matches!(remove_link(&dst), Err(LinkError::NotALink(_))));
        assert!(dst.join("mine.txt").is_file(), "外部目录不得被删");
    }
}
