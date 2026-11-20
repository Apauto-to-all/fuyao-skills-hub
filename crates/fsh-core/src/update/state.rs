//! 检查状态（hub 根 `update.toml`，机器写、ignored）：上次检查时间与上次查到的最新版本。
//! 配置是**人写的**、状态是**机器写的**，两者必须分家——时间戳写进 tracked 的
//! `hub.toml` 就会触发自动提交。它天然被 `.gitignore` 白名单的 `/*` 挡在仓库外。

use std::fs;

use serde::{Deserialize, Serialize};

use crate::paths::HubPaths;

/// 版本检查状态
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdateState {
    /// 上次检查时间（Unix 秒）；`0` 表示从未查过
    #[serde(default)]
    pub last_check: u64,
    /// 上次查到的最新版本；查不到或解析不了时缺席
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
}

impl UpdateState {
    /// 读状态文件；缺失或写坏都当「从未查过」——状态坏了最多多查一次，
    /// 不该让命令失败，也不值得为它报错
    pub fn load(paths: &HubPaths) -> Self {
        match fs::read_to_string(paths.update_toml()) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// 节流判定：距今不足 `interval_hours` 即算新鲜，不发请求。
    /// `0` 表示每条命令都查，于是永远不新鲜
    pub fn is_fresh(&self, now: u64, interval_hours: u64) -> bool {
        if interval_hours == 0 {
            return false;
        }
        // 时钟回拨（now 早于 last_check）算作新鲜：宁可少查一次，也不因
        // 系统时间变动把每条命令都拖去联网
        now.saturating_sub(self.last_check) < interval_hours.saturating_mul(3600)
    }

    /// 写回状态文件；写失败只记一条 WARN，不影响命令结果
    pub fn save(&self, paths: &HubPaths) {
        let target = paths.update_toml();
        let written = toml::to_string(self)
            .map_err(|e| e.to_string())
            .and_then(|text| fs::write(&target, text).map_err(|e| e.to_string()));
        if let Err(cause) = written {
            tracing::warn!(path = %target.display(), cause = %cause, "版本检查状态写入失败");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, HubPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = HubPaths::from_root(dir.path());
        (dir, paths)
    }

    #[test]
    fn missing_state_reads_as_never_checked() {
        let (_dir, paths) = paths();
        assert_eq!(UpdateState::load(&paths), UpdateState::default());
    }

    #[test]
    fn state_round_trips_through_disk() {
        let (_dir, paths) = paths();
        let state = UpdateState {
            last_check: 1_700_000_000,
            latest: Some("0.1.3".to_string()),
        };
        state.save(&paths);
        assert_eq!(UpdateState::load(&paths), state);
    }

    #[test]
    fn latest_absent_when_never_found() {
        let (_dir, paths) = paths();
        UpdateState {
            last_check: 42,
            latest: None,
        }
        .save(&paths);
        let text = fs::read_to_string(paths.update_toml()).unwrap();
        assert!(
            !text.contains("latest"),
            "没有缓存版本时不该写出空键: {text}"
        );
        assert_eq!(UpdateState::load(&paths).last_check, 42);
    }

    #[test]
    fn broken_state_reads_as_never_checked() {
        let (_dir, paths) = paths();
        fs::write(paths.update_toml(), "last_check = ").unwrap();
        assert_eq!(UpdateState::load(&paths), UpdateState::default());
    }

    #[test]
    fn throttle_holds_inside_the_interval() {
        let state = UpdateState {
            last_check: 1_000,
            latest: None,
        };
        // 23 小时后仍新鲜，25 小时后过期
        assert!(state.is_fresh(1_000 + 23 * 3600, 24));
        assert!(!state.is_fresh(1_000 + 25 * 3600, 24));
    }

    #[test]
    fn zero_interval_is_never_fresh() {
        let state = UpdateState {
            last_check: 1_000,
            latest: None,
        };
        assert!(!state.is_fresh(1_000, 0), "0 表示每条命令都查");
    }

    #[test]
    fn clock_going_backwards_stays_fresh() {
        let state = UpdateState {
            last_check: 5_000,
            latest: None,
        };
        assert!(state.is_fresh(1_000, 24), "时钟回拨不该触发联网");
    }
}
