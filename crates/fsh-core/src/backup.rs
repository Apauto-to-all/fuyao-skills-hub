//! 备份：把 data\ 全量打成带时间戳的 zip 单文件，堵住远端同步（M3）之前
//! 的单副本空窗。恢复 = 手动解压覆盖安装目录（M0 不做 restore 命令）。

use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::hub;
use crate::paths::HubPaths;

/// 备份错误
#[derive(Debug, Error)]
pub enum BackupError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] io::Error),
    /// zip 打包 / 解包失败
    #[error("zip 错误: {0}")]
    Zip(#[from] zip::result::ZipError),
    /// 登记簿读写失败（记录备份时间戳）
    #[error("登记簿错误: {0}")]
    Registry(#[from] hub::HubError),
}

/// 把整个 data\ 打包为 dest_dir 下的 fuyao-backup-YYYYMMDD-HHMMSS.zip，
/// 返回生成的 zip 路径。排除 *.lock 临时文件（git 索引锁等）；
/// 成功后把时间戳写入登记簿（doctor 的 N 天未备份提醒依据）。
pub fn backup(hub_root: &Path, dest_dir: &Path) -> Result<PathBuf, BackupError> {
    fs::create_dir_all(dest_dir)?;
    let now = hub::unix_now();
    let zip_path = dest_dir.join(format!("fuyao-backup-{}.zip", format_timestamp(now)));

    let file = File::create(&zip_path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    add_dir(&mut writer, hub_root, hub_root, options)?;
    writer.finish()?;

    // 记录备份时间：doctor 据此提醒「超过 N 天未备份」
    let paths = HubPaths::from_root(hub_root);
    let mut registry = hub::load_registry(&paths)?;
    registry.last_backup = now;
    hub::save_registry(&paths, &registry)?;

    Ok(zip_path)
}

/// 递归把 base 下的全部文件写进 zip（条目名 = 相对 base 的正斜杠路径）；
/// *.lock 是运行时临时锁，跳过以保证恢复出的仓库干净可用
fn add_dir(
    writer: &mut ZipWriter<File>,
    base: &Path,
    dir: &Path,
    options: SimpleFileOptions,
) -> Result<(), BackupError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            add_dir(writer, base, &p, options)?;
        } else if p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lock"))
        {
            continue;
        } else {
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            writer.start_file(rel, options)?;
            io::copy(&mut File::open(&p)?, writer)?;
        }
    }
    Ok(())
}

/// Unix 秒 → UTC「YYYYMMDD-HHMMSS」。时区取 UTC：备份时刻只用于区分
/// 文件名先后，不需要本地墙钟
fn format_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 天数 → 公历年月日的确定性换算（自 1970-01-01 起的累积天数，
/// 先平移到以 0000-03-01 为原点的坐标，再用 400 年/100 年/4 年周期收拢）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::ZipArchive;

    #[test]
    fn backup_zips_everything_but_locks_and_stamps_time() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        let hub_root = paths.root();
        // 一个会被打包的普通文件 + 一个必须被排除的锁文件
        fs::write(hub_root.join("probe.txt"), "hello").unwrap();
        fs::write(hub_root.join("debug.lock"), "locked").unwrap();

        let dest = tempfile::tempdir().unwrap();
        let zip_path = backup(hub_root, dest.path()).unwrap();
        // 文件名带时间戳
        let name = zip_path.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            name.starts_with("fuyao-backup-"),
            "名字应以 fuyao-backup- 开头: {name}"
        );
        assert!(name.ends_with(".zip"));

        // zip 可开、条目齐全、锁文件被排除
        let file = File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "hub.toml"), "配置应在包内");
        assert!(names.iter().any(|n| n == ".git/HEAD"), ".git 真身应在包内");
        assert!(names.iter().any(|n| n == "probe.txt"));
        assert!(
            !names.iter().any(|n| n.ends_with(".lock")),
            "锁文件必须排除"
        );

        // 备份时间戳写入登记簿
        let registry = hub::load_registry(&HubPaths::from_root(hub_root)).unwrap();
        assert!(registry.last_backup > 0, "登记簿应记录备份时间");
    }

    #[test]
    fn timestamp_formats_as_utc_datetime() {
        // 2026-10-29 00:00:00 UTC = 1793232000
        assert_eq!(format_timestamp(1_793_232_000), "20261029-000000");
        // 1970-01-01 00:00:01
        assert_eq!(format_timestamp(1), "19700101-000001");
    }
}
