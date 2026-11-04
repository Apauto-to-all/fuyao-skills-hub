//! 日志装配：subscriber 分层、非阻塞写入与日志文件保留。
//!
//! 宿主（CLI / 桌面端）启动时各调用一次 [`init_logging`]：文件层写 hub 根下的
//! `logs\`（按天滚动，只保留最近 [`RETENTION_DAYS`] 天），stderr 层始终输出。
//! 文件层失败只降级为 stderr，绝不让宿主启动失败。

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    EnvFilter, Layer, Registry, fmt, fmt::time::ChronoLocal, layer::SubscriberExt,
};

use crate::paths::HubPaths;

/// 日志文件保留天数：过期文件在下次启动装配时清掉
const RETENTION_DAYS: u64 = 30;

/// 日志文件名前缀；滚动后的文件名形如 `fsh.2026-11-05.log`
const FILE_PREFIX: &str = "fsh";

/// 初始化日志：装配「文件 + stderr」双层 subscriber 并设为全局默认。
///
/// 级别固定 `info`，环境变量 `RUST_LOG` 存在时以它为准。
/// 全进程只能生效一次：`set_global_default` 重复调用静默忽略。
///
/// 返回的 guard 持有后台写入线程，**必须存活到宿主进程结束**：它 drop 时把
/// 缓冲里剩下的日志刷到磁盘。宿主写成 `let _log_guard = init_logging(..)` 即可。
pub fn init_logging(paths: &HubPaths) -> WorkerGuard {
    // RUST_LOG 优先；未设或解析失败则用固定 info
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // 本地时区计时：日志时间与用户时钟一致
    let timer = ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f".to_string());

    // 文件层装配失败时换成丢弃写入器：guard 照常返回，日志只剩 stderr
    let (writer, guard) = match make_file_writer(paths) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("[fsh] 日志文件初始化失败，仅输出到 stderr: {e}");
            tracing_appender::non_blocking(io::sink())
        }
    };

    let layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = vec![
        Box::new(
            fmt::layer()
                .with_writer(writer)
                // 文件里不要 ANSI 颜色码
                .with_ansi(false)
                .with_timer(timer.clone())
                .with_filter(filter.clone()),
        ),
        Box::new(
            fmt::layer()
                .with_writer(io::stderr)
                .with_ansi(true)
                .with_timer(timer)
                .with_filter(filter),
        ),
    ];

    let subscriber = Registry::default().with(layers);
    let _ = tracing::subscriber::set_global_default(subscriber);

    guard
}

/// 构建日志文件写入器：logs 目录按需创建、清掉过期文件，按本地日期滚动的写入器包成非阻塞写入。
fn make_file_writer(paths: &HubPaths) -> io::Result<(NonBlocking, WorkerGuard)> {
    let dir = paths.logs();
    std::fs::create_dir_all(&dir)?;
    sweep_expired(&dir);

    let writer = DailyLog::open(dir)?;
    Ok(tracing_appender::non_blocking(writer))
}

/// 按本地日期滚动的日志文件：每次写入前比对日期，跨天就换文件。
///
/// 文件读写全部交给 `RollingFileAppender`（`Rotation::NEVER` + 带日期的前缀），
/// 这里只负责日期取自本地时区：日志行的时间是本地时间，文件名若按 UTC 算，
/// 本地凌晨会落进前一天的文件里，文件名与内容对不上。
struct DailyLog {
    /// 日志目录；跨天时在同一目录里换新文件
    dir: PathBuf,
    /// 当前文件对应的本地日期（`YYYY-MM-DD`）
    date: String,
    /// 官方滚动 appender：负责打开、追加与写入
    appender: RollingFileAppender,
}

impl DailyLog {
    /// 打开当天的日志文件（追加），目录须已存在
    fn open(dir: PathBuf) -> io::Result<Self> {
        let date = local_date();
        let appender = Self::appender_for(&dir, &date)?;
        Ok(Self {
            dir,
            date,
            appender,
        })
    }

    /// 构造写指定日期文件的 appender；`Rotation::NEVER` 下文件名即「前缀.后缀」
    fn appender_for(dir: &Path, date: &str) -> io::Result<RollingFileAppender> {
        RollingFileAppender::builder()
            .rotation(Rotation::NEVER)
            .filename_prefix(format!("{FILE_PREFIX}.{date}"))
            .filename_suffix("log")
            .build(dir)
            .map_err(io::Error::other)
    }

    /// 跨天换文件；新文件开不出来时继续写旧文件，宁可文件名差一天也不丢日志
    fn rotate_if_stale(&mut self) {
        let today = local_date();
        if today == self.date {
            return;
        }
        match Self::appender_for(&self.dir, &today) {
            Ok(appender) => {
                self.appender = appender;
                self.date = today;
            }
            Err(e) => eprintln!("[fsh] 日志跨天换文件失败，继续写旧文件: {e}"),
        }
    }
}

impl Write for DailyLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.rotate_if_stale();
        self.appender.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.appender.flush()
    }
}

/// 本地日期（`YYYY-MM-DD`）：文件名与日志行共用同一个时钟
fn local_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 删除超过保留期的日志文件；目录读不了就静默跳过，下次启动再试。
fn sweep_expired(dir: &Path) {
    let now = SystemTime::now();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // 只碰自己产的日志文件，目录里别的东西一律不动
        if !name.starts_with(FILE_PREFIX) || !name.ends_with(".log") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if is_expired(modified, now) {
            // 删不掉不算错误（可能正被占用），下次启动再清
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// 修改时间早于保留窗口即为过期；时钟回拨导致的负年龄按未过期处理。
fn is_expired(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .map(|age| age > Duration::from_secs(RETENTION_DAYS * 86_400))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 保留窗口内外的判定：29 天未过期，31 天过期，未来时间不算过期。
    #[test]
    fn expiry_window_is_thirty_days() {
        let now = SystemTime::now();
        assert!(!is_expired(now - Duration::from_secs(29 * 86_400), now));
        assert!(is_expired(now - Duration::from_secs(31 * 86_400), now));
        assert!(!is_expired(now + Duration::from_secs(3600), now));
    }

    /// 清扫只删过期日志：新日志与非日志文件都留着。
    #[test]
    fn sweep_removes_only_expired_logs() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("fsh.2026-01-01.log");
        let fresh = temp.path().join("fsh.2026-11-05.log");
        let other = temp.path().join("notes.log");
        for path in [&old, &fresh, &other] {
            std::fs::write(path, b"x").unwrap();
        }
        let ancient = SystemTime::now() - Duration::from_secs(40 * 86_400);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(ancient)
            .unwrap();

        sweep_expired(temp.path());

        assert!(!old.exists(), "过期日志应被删除");
        assert!(fresh.exists(), "保留期内日志不应被删");
        assert!(other.exists(), "非本模块产出的文件不应被删");
    }

    /// 写入器装配会建出 logs 目录，guard drop 只 flush 缓冲不 panic。
    #[test]
    fn writer_creates_logs_dir() {
        let temp = tempfile::tempdir().unwrap();
        let paths = HubPaths::from_root(temp.path());

        let (writer, guard) = make_file_writer(&paths).unwrap();
        assert!(paths.logs().is_dir(), "logs 目录应被创建");
        drop(writer);
        drop(guard);
    }

    /// 文件名用本地日期，与日志行里的时间同一天
    #[test]
    fn writer_names_file_with_local_date() {
        let temp = tempfile::tempdir().unwrap();
        let paths = HubPaths::from_root(temp.path());

        let (mut writer, guard) = make_file_writer(&paths).unwrap();
        writer.write_all(b"x\n").unwrap();
        let expected = paths
            .logs()
            .join(format!("{FILE_PREFIX}.{}.log", local_date()));
        assert!(expected.is_file(), "日志文件名应用本地日期: {expected:?}");
        drop(writer);
        drop(guard);
    }

    /// logs 位置被同名文件占住时装配返回错误而不是 panic：调用方据此降级为纯 stderr。
    #[test]
    fn writer_fails_when_logs_path_is_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let blocked = temp.path().join("logs");
        std::fs::write(&blocked, b"x").unwrap();
        let paths = HubPaths::from_root(temp.path());

        assert!(make_file_writer(&paths).is_err(), "目录建不出来应报错");
    }

    /// 过期但删不掉的文件（Windows 只读）留着不 panic，下次启动再试。
    #[cfg(windows)]
    #[test]
    // Windows 上只有 set_readonly(false) 能清掉只读属性，临时目录才能整体删除
    #[allow(clippy::permissions_set_readonly_false)]
    fn sweep_skips_undeletable_log() {
        let temp = tempfile::tempdir().unwrap();
        let locked = temp.path().join("fsh.2026-01-01.log");
        std::fs::write(&locked, b"x").unwrap();
        let ancient = SystemTime::now() - Duration::from_secs(40 * 86_400);
        let file = std::fs::File::options().write(true).open(&locked).unwrap();
        file.set_modified(ancient).unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&locked, perms).unwrap();
        drop(file);

        sweep_expired(temp.path());

        // 删得掉与否都不影响清扫本身：只要不 panic、不动别的文件即可
        if locked.exists() {
            let mut perms = std::fs::metadata(&locked).unwrap().permissions();
            perms.set_readonly(false);
            std::fs::set_permissions(&locked, perms).unwrap();
        }
    }
}
