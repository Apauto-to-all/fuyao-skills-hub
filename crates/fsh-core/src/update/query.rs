//! 发布源查询：GET `<registry>/-/package/<包名>/dist-tags`，取 `channel` 对应的值。
//! 选这个端点而非完整 manifest：响应约 20 字节、无速率限制、无需 User-Agent。

use std::collections::BTreeMap;
use std::time::Duration;

use crate::hub::UpdateConfig;

use super::PACKAGE;

/// 查询超时：被动检查挂在命令末尾，宁可放弃也不能把命令拖慢
const TIMEOUT: Duration = Duration::from_secs(3);

/// 查 `channel` 对应的最新版本；网络失败、非 200、响应解析失败、缺这个 tag
/// 一律返回错误，由调用方静默放弃
pub fn fetch_latest(cfg: &UpdateConfig) -> Result<String, String> {
    let url = format!(
        "{}/-/package/{PACKAGE}/dist-tags",
        cfg.registry.trim_end_matches('/')
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let body: String = agent
        .get(&url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    tags(&body)?
        .remove(&cfg.channel)
        .ok_or_else(|| format!("发布源没有 {} 这个 tag", cfg.channel))
}

/// dist-tags 响应体：tag 名 → 版本串
fn tags(body: &str) -> Result<BTreeMap<String, String>, String> {
    serde_json::from_str(body).map_err(|e| format!("发布源响应解析失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_configured_channel() {
        let body = r#"{"latest":"0.1.2","next":"0.2.0-beta.1"}"#;
        assert_eq!(tags(body).unwrap().remove("latest").unwrap(), "0.1.2");
        assert_eq!(
            tags(body).unwrap().remove("next").unwrap(),
            "0.2.0-beta.1",
            "channel 换 tag 名即可，不用改代码"
        );
    }

    #[test]
    fn rejects_non_json_body() {
        assert!(tags("<html>").is_err());
    }

    #[test]
    fn missing_channel_is_an_error() {
        assert!(
            tags(r#"{"latest":"0.1.2"}"#)
                .unwrap()
                .remove("next")
                .is_none()
        );
    }
}
