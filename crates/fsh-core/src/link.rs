//! 目录链接引擎：把「源目录以链接形式出现在目标位置」抽象为一个原语。
//! 三级降级（Windows 实况）：目录符号链接 → NTFS junction → 纯拷贝。
//! 实际采用了哪一级记在登记簿的 link 字段，项目里不留任何 fsh 文件。

use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::error::{CommonError, common_error_from};
use crate::skill::{Occupancy, occupancy};

/// 链接错误
#[derive(Debug, Error)]
pub enum LinkError {
    /// 底层失败（文件系统）
    #[error(transparent)]
    Common(#[from] CommonError),
    /// 目标位置已存在非本引擎管理的目录，拒绝覆盖
    #[error("目标已存在且不受管理: {0}")]
    Occupied(String),
    /// 钉住的档位在本机办不到：不换别的档位顶替
    #[error("本机无法建立{kind}入口: {path}（{cause}）")]
    Pinned {
        /// 钉住的档位
        kind: LinkKind,
        /// 建不出的落点
        path: String,
        /// 底层失败原因
        cause: String,
    },
}

common_error_from!(LinkError: std::io::Error);

impl ErrorCode for LinkError {
    fn error_code(&self) -> &'static str {
        match self {
            LinkError::Common(c) => c.error_code(),
            LinkError::Occupied(_) => "occupied",
            LinkError::Pinned { .. } => "link_failed",
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

impl LinkKind {
    /// 三个档位：词表的唯一来源。参数解析、错误消息与测试都从这里取，
    /// 新增一档不用再摸一遍各处清单
    pub const ALL: [LinkKind; 3] = [LinkKind::Symlink, LinkKind::Junction, LinkKind::Copy];

    /// 线上词形：登记簿的 `link` 值、`--json` 的 `link` 字段、`fsh list` 的链接方式列
    /// 与 `--link` 的取值共用这一个词表
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Symlink => "symlink",
            LinkKind::Junction => "junction",
            LinkKind::Copy => "copy",
        }
    }

    /// 从线上词形解析：只认三个小写英文词，不做大小写或别名归一
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == name)
    }
}

/// 在 dst 处建立指向 src 的目录链接：按符号链接 → junction → 拷贝
/// 依次尝试，返回实际采用的方式。dst 已存在时拒绝。
pub fn create_dir_link(src: &Path, dst: &Path) -> Result<LinkKind, LinkError> {
    prepare(dst)?;
    // 一级：目录符号链接（Windows 需开发者模式或提权，失败即降级）
    if build(src, dst, LinkKind::Symlink).is_ok() {
        // 决策：降级链上实际采用哪一级，排查「为何不同步」时先看这条
        tracing::debug!(path = %dst.display(), kind = %"symlink", "链接方式选择");
        return Ok(LinkKind::Symlink);
    }
    // 二级：NTFS junction（无门槛主力）
    if build(src, dst, LinkKind::Junction).is_ok() {
        tracing::debug!(path = %dst.display(), kind = %"junction", "链接方式选择");
        return Ok(LinkKind::Junction);
    }
    // 三级：纯拷贝（无任何附带文件）
    build(src, dst, LinkKind::Copy)?;
    // 降到拷贝是最后一级，意味着链接能力整体不可用，必须留痕
    tracing::debug!(path = %dst.display(), kind = %"copy", "链接方式选择");
    Ok(LinkKind::Copy)
}

/// 在 dst 处按指定的档位建立指向 src 的入口：办不到即报错并撤净残留，
/// 不换别的档位顶替。档位来自调用方——`--link` 给的、或登记簿那条记录定下的，
/// 盘上形态因此与所定档位永远同一种
pub fn create_dir_link_pinned(src: &Path, dst: &Path, kind: LinkKind) -> Result<(), LinkError> {
    prepare(dst)?;
    match build(src, dst, kind) {
        Ok(()) => {
            tracing::debug!(path = %dst.display(), kind = ?kind, "按指定档位建入口");
            Ok(())
        }
        Err(e) => {
            // 拷贝可能写到一半：半棵树不是可用入口，也不留给回滚去猜
            if kind == LinkKind::Copy {
                let _ = fs::remove_dir_all(dst);
            }
            Err(LinkError::Pinned {
                kind,
                path: dst.display().to_string(),
                cause: e.to_string(),
            })
        }
    }
}

/// 落点前置：任何已存在的目标都拒绝（只在自己管理的空位上落入口），父层建出
fn prepare(dst: &Path) -> Result<(), LinkError> {
    if occupancy(dst) != Occupancy::Nothing {
        return Err(LinkError::Occupied(dst.display().to_string()));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// 落一级入口：成功即该档位成立。降级链与钉住的档位都经这一处落盘
fn build(src: &Path, dst: &Path, kind: LinkKind) -> std::io::Result<()> {
    match kind {
        LinkKind::Symlink => symlink_dir(src, dst),
        LinkKind::Junction => junction::create(src, dst),
        LinkKind::Copy => copy_dir_recursive(src, dst),
    }
}

/// 目录符号链接（Windows 需开发者模式或提权）
#[cfg(windows)]
fn symlink_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
}

/// 本引擎的链接能力只对 Windows 目录成立，别处不另开一条降级链
#[cfg(not(windows))]
fn symlink_dir(_src: &Path, _dst: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("本引擎只在 Windows 上建目录符号链接"))
}

/// 撤销链接：按登记簿记录的档位撤——链接与 junction 只删自身，拷贝整
/// 目录删。目标不存在视为已撤销（幂等）。
pub fn remove_link(dst: &Path, kind: LinkKind) -> Result<(), LinkError> {
    if occupancy(dst) == Occupancy::Nothing {
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

    /// 钉住拷贝：本机链接能力可用也照样落真目录，不挑降级链上更省事的那一级
    #[test]
    fn pinned_copy_stays_a_real_directory() {
        let src = make_src();
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("copied");
        create_dir_link_pinned(src.path(), &dst, LinkKind::Copy).unwrap();
        assert_on_disk(LinkKind::Copy, &dst);
        assert!(dst.join("refs/a.md").is_file(), "拷贝要带全内容");
        remove_link(&dst, LinkKind::Copy).unwrap();
        assert!(!dst.exists(), "按拷贝档撤销要删净");
    }

    /// 钉住 junction：落点表现为 reparse point 且穿透到源
    #[test]
    fn pinned_junction_builds_a_reparse_point() {
        let src = make_src();
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("joined");
        create_dir_link_pinned(src.path(), &dst, LinkKind::Junction).unwrap();
        assert_on_disk(LinkKind::Junction, &dst);
        assert!(dst.join("SKILL.md").is_file());
        remove_link(&dst, LinkKind::Junction).unwrap();
        assert!(!dst.exists());
    }

    /// 钉住的档位办不到即报错，且不留半截落点：整笔回滚从干净状态开始
    #[test]
    fn pinned_failure_leaves_no_destination() {
        let src = make_src();
        let missing = src.path().join("no-such-sub");
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("half");
        let err = create_dir_link_pinned(&missing, &dst, LinkKind::Copy).unwrap_err();
        assert_eq!(err.error_code(), "link_failed");
        assert!(!dst.exists(), "拷到一半失败要删净，不留半棵树");
    }

    /// 钉住的档位与盘上占用无关：目标已存在时按占用拒绝，不算档位办不到
    #[test]
    fn pinned_link_refuses_occupied_dst() {
        let src = make_src();
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("occupied");
        fs::create_dir_all(&dst).unwrap();
        assert!(matches!(
            create_dir_link_pinned(src.path(), &dst, LinkKind::Junction),
            Err(LinkError::Occupied(_))
        ));
    }

    /// 线上词形双向一致：`--link` 收的词与登记簿、`fsh list` 打出来的词是同一套
    #[test]
    fn wire_names_round_trip_and_reject_strangers() {
        for kind in LinkKind::ALL {
            assert_eq!(LinkKind::from_name(kind.as_str()), Some(kind));
        }
        assert_eq!(LinkKind::from_name("soft"), None, "别名不认");
        assert_eq!(LinkKind::from_name("Copy"), None, "大小写不归一");
        assert_eq!(LinkKind::from_name(""), None);
    }

    #[test]
    fn remove_link_is_idempotent_on_missing() {
        let dst_parent = tempfile::tempdir().unwrap();
        let dst = dst_parent.path().join("never-existed");
        for kind in LinkKind::ALL {
            remove_link(&dst, kind).unwrap();
        }
    }
}
