//! 版本检查：被动、只读地查一次发布源，看有没有比当前程序更新的版本。
//! 按节流间隔最多查一次，查不到或连不上都静默；发现新版本只产出一行升级
//! 提示交给调用方打到 stderr，绝不改命令的输出与退出码。
//! 本模块为目录模块：配置读取（config）、状态读写与节流（state）、
//! 发布源查询（query）、版本比较（version）各自独立，此处只留对外入口与提示成形。

use std::path::Path;

use crate::hub::{self, UpdateConfig, UpdateMode};
use crate::paths::HubPaths;

mod config;
mod query;
mod state;
mod version;

pub use config::load_update_config;
pub use query::fetch_latest;
pub use state::UpdateState;
pub use version::newer_than;

/// 发布源的包名：与升级提示里那条 `npm i -g` 的包名同一个，提示才不会说假话
pub(crate) const PACKAGE: &str = "fuyao-skills-hub";

/// 当前程序版本：编译期固化，与升级提示里的「当前」同一个来源
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 进程侧的事实：提示与否还要看它们，但都不是本模块能自己知道的
#[derive(Debug, Clone, Copy)]
pub struct Gate {
    /// `--json` 是否开着：机器信封必须保持纯净
    pub json: bool,
    /// stderr 是否连着终端
    pub stderr_tty: bool,
    /// `CI` 环境变量是否非空
    pub in_ci: bool,
}

/// 升级提示：单行，agent 能用一条正则抓住它
pub fn notice_line(latest: &str, current: &str) -> String {
    format!("提示: {PACKAGE} 有新版本 {latest}（当前 {current}），升级: npm i -g {PACKAGE}@latest")
}

/// 被动检查的完整判定：全部抑制条件都过了才可能联网，查到更新的版本才给提示。
/// `code` 是命令的退出码——非 0 的命令不带提示
pub fn passive_notice(hub_root: &Path, code: i32, gate: &Gate) -> Option<String> {
    if code != 0 {
        return None;
    }
    let paths = HubPaths::from_root(hub_root);
    if !paths.is_initialized() {
        return None;
    }
    let cfg = load_update_config(&paths);
    if cfg.mode != UpdateMode::Auto {
        return None;
    }
    if gate.json {
        return None;
    }
    // 输出被重定向时默认闭嘴：agent 的上下文不该因为每条命令多一行而膨胀
    if !gate.stderr_tty && !cfg.notify_when_piped {
        return None;
    }
    if gate.in_ci && !cfg.notify_in_ci {
        return None;
    }
    let latest = cached_or_refresh(&paths, &cfg)?;
    newer_than(&latest, CURRENT_VERSION).then(|| notice_line(&latest, CURRENT_VERSION))
}

/// 节流判定与刷新：缓存新鲜就直接用缓存，过期才联网。
/// 失败的检查也更新检查时间，但不覆盖已有的最新版本
fn cached_or_refresh(paths: &HubPaths, cfg: &UpdateConfig) -> Option<String> {
    cached_or_refresh_with(paths, cfg, || fetch_latest(cfg))
}

/// 同上的注入形态：查询由调用方给出，好让「查到了但不比当前新」这条
/// 分支不必真起一个发布源就能验
fn cached_or_refresh_with(
    paths: &HubPaths,
    cfg: &UpdateConfig,
    fetch: impl FnOnce() -> Result<String, String>,
) -> Option<String> {
    let now = hub::unix_now();
    let mut state = UpdateState::load(paths);
    if state.is_fresh(now, cfg.interval_hours) {
        return state.latest;
    }
    let started = std::time::Instant::now();
    match fetch() {
        Ok(latest) => {
            tracing::info!(
                latest = %latest,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "版本检查完成"
            );
            // 发布源不比当前新时保留已有缓存：宁可留着上次查到的高版本，
            // 也不把缓存降级成一条「已过期」的结论
            if newer_than(&latest, CURRENT_VERSION) {
                state.latest = Some(latest);
            }
        }
        Err(e) => {
            tracing::warn!(
                cause = %e,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "版本检查失败"
            );
        }
    }
    // 成败都记下这次检查：断网时一天最多卡一次超时，而不是每条命令都卡
    state.last_check = now;
    state.save(paths);
    state.latest
}

#[cfg(test)]
mod tests;
