//! 目录链接引擎：把「源目录以链接形式出现在目标位置」抽象为一个原语。
//! 三级降级（Windows 实况）：目录符号链接 → NTFS junction → 纯拷贝。
//! 实际采用了哪一级记在登记簿的 link 字段，项目里不留任何 fsh 文件。

use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;

/// 链接错误
#[derive(Debug, Error)]
pub enum LinkError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// 目标位置已存在非本引擎管理的目录，拒绝覆盖
    #[error("目标已存在且不受管理: {0}")]
    Occupied(String),
}

impl ErrorCode for LinkError {
    fn error_code(&self) -> &'static str {
        match self {
            LinkError::Io(_) => "io",
            LinkError::Occupied(_) => "occupied",
        }
    }
}

/// 实际采用的链接方式（三级降级的结果）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// 目录符号链接（需开发者模式或提权）
    Symlink,
    /// NTFS junction（无门槛，主力）
    #[default]
    Junction,
    /// 纯拷贝：不跟源同步，撤销按整目录删
    Copy,
}

/// 人读名：诊断说明与界面共用一套叫法——中文打头、括号里带英文原名，
/// 两种读者都能对上号
impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LinkKind::Symlink => "符号链接（Symlink）",
            LinkKind::Junction => "目录联接（Junction）",
            LinkKind::Copy => "拷贝（Copy）",
        })
    }
}

/// 在 dst 处建立指向 src 的目录链接：按符号链接 → junction → 拷贝
/// 依次尝试，返回实际采用的方式。dst 已存在时拒绝。
pub fn create_dir_link(src: &Path, dst: &Path) -> Result<LinkKind, LinkError> {
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
            // 决策：降级链上实际采用哪一级，排查「为何不同步」时先看这条
            tracing::debug!(path = %dst.display(), kind = %"symlink", "链接方式选择");
            return Ok(LinkKind::Symlink);
        }
    }
    // 二级：NTFS junction（无门槛主力）
    if junction::create(src, dst).is_ok() {
        tracing::debug!(path = %dst.display(), kind = %"junction", "链接方式选择");
        return Ok(LinkKind::Junction);
    }
    // 三级：纯拷贝（无任何附带文件）
    copy_dir_recursive(src, dst)?;
    // 降到拷贝是最后一级，意味着链接能力整体不可用，必须留痕
    tracing::debug!(path = %dst.display(), kind = %"copy", "链接方式选择");
    Ok(LinkKind::Copy)
}

/// 撤销链接：按登记簿记录的档位撤——链接与 junction 只删自身，拷贝整
/// 目录删。目标不存在视为已撤销（幂等）。
pub fn remove_link(dst: &Path, kind: LinkKind) -> Result<(), LinkError> {
    if dst.symlink_metadata().is_err() {
        return Ok(());
    }
    match kind {
        LinkKind::Symlink | LinkKind::Junction => fs::remove_dir(dst)?,
        LinkKind::Copy => fs::remove_dir_all(dst)?,
    }
    Ok(())
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
            }
        }
    }

    #[test]
    fn create_link_falls_back_and_is_readable() {
        let src = make_src();
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("linked");
        let kind = create_dir_link(src.path(), &dst).unwrap();
        assert_on_disk(kind, &dst);
        // 无论哪一级，「读 dst 应能看到源文件」
        assert!(dst.join("SKILL.md").is_file());
        assert!(dst.join("refs/a.md").is_file());
        // 目标里只能出现源目录已有的条目，不留下任何额外文件
        for entry in fs::read_dir(&dst).unwrap() {
            let name = entry.unwrap().file_name();
            assert!(
                src.path().join(&name).exists(),
                "目标里出现源目录没有的条目"
            );
        }
        // 撤销后干净
        remove_link(&dst, kind).unwrap();
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
            create_dir_link(src.path(), &dst),
            Err(LinkError::Occupied(_))
        ));
        // 用户文件不得被动
        assert!(dst.join("user-file.txt").is_file());
    }

    #[test]
    fn remove_link_is_idempotent_on_missing() {
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("never-existed");
        for kind in [LinkKind::Symlink, LinkKind::Junction, LinkKind::Copy] {
            remove_link(&dst, kind).unwrap();
        }
    }
}
