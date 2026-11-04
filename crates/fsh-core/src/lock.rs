//! 进程级文件锁：hub 全部写入口共用的全局单写锁。
//! 锁文件落 export\\ 根下（ignored、可整体删除），用 OS 文件锁（Windows
//! LockFileEx / Unix flock）实现独占：进程退出或句柄关闭即自动释放，
//! 残留空文件无任何状态、下次直接复用，无需陈锁清理。
//! 锁内顺序不变量：先取锁（write_lock 取得 WriteToken）→ 再读登记簿 → 最后写盘。

use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use thiserror::Error;

use crate::ErrorCode;
use crate::paths::HubPaths;

/// 文件锁错误
#[derive(Debug, Error)]
pub enum LockError {
    /// 锁文件打开 / 操作失败
    #[error("文件锁 IO 错误: {0}")]
    Io(#[from] std::io::Error),
    /// 锁已被其他持有者占用（进程或 GUI 并发写）
    #[error("hub 正被其他进程写入（文件锁被占用）")]
    Held,
}

impl ErrorCode for LockError {
    fn error_code(&self) -> &'static str {
        match self {
            LockError::Io(_) => "io",
            LockError::Held => "lock_held",
        }
    }
}

/// 写路径凭证：持有即证明当前进程已持有 hub 全局写锁，Drop 时
/// 句柄关闭、OS 自动释放。字段私有，仅本模块的 write_lock 能构造；
/// hub 写路径的共享写函数以本类型为参数，未取锁的调用在编译期即被拒绝。
pub struct WriteToken {
    /// 持有打开的锁文件句柄；锁随句柄生存
    _file: File,
}

impl WriteToken {
    /// 锁文件路径（export\\.lock）
    fn lock_path(paths: &HubPaths) -> PathBuf {
        paths.export().join(".lock")
    }
}

/// 获取 hub 全局写锁：成功返回写凭证，被占用返回 LockError::Held。
/// 全部 hub 写入口（入库 / 发版 / 归档 / 彻底删除 / 恢复 / gc / init）
/// 必须先拿到本锁再动任何 hub 数据；锁内顺序：先取锁 → 再读登记簿 → 后写盘。
// ponytail: 全库一把全局写锁，个人工具无写并发场景；
// 若未来出现高频并发写，升级为按 skill 粒度分锁
pub fn write_lock(paths: &HubPaths) -> Result<WriteToken, LockError> {
    let path = WriteToken::lock_path(paths);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 不用 create_new：残留锁文件是常态（上次进程的遗留），直接复用；
    // truncate(false) 显式声明：已存在的锁文件内容保持原样（本来就是空文件）
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    // 独占锁非阻塞获取：拿不到即报占用，绝不排队等待
    match file.try_lock() {
        Ok(()) => Ok(WriteToken { _file: file }),
        Err(e) => {
            // 决策：锁被占用是可恢复的并发情形，记 WARN 供排查「谁在写」；不带锁文件内容
            tracing::warn!(path = %path.display(), cause = %e, "获取写锁失败");
            Err(LockError::Held)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 已持锁时再次获取必须失败；释放后同一路径可重新获取。
    /// 构造封闭性由编译期保证：字段私有，模块外造不出 WriteToken，
    /// 取锁唯此一门。
    #[test]
    fn write_lock_is_exclusive_and_reusable() {
        let install = tempfile::tempdir().unwrap();
        let paths = HubPaths::new(install.path());
        std::fs::create_dir_all(paths.export()).unwrap();

        let g1 = write_lock(&paths).unwrap();
        // 第二个持有者（模拟并发进程 / GUI）必须被拒
        assert!(matches!(write_lock(&paths), Err(LockError::Held)));
        drop(g1);
        // 释放后锁文件原地复用，再次获取成功
        assert!(write_lock(&paths).is_ok());
    }
}
