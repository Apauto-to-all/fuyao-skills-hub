//! hub.toml：库级配置的类型与读写，随 hub 仓库 tracked。

use std::fs;

use serde::{Deserialize, Serialize};

use super::HubError;
use crate::paths::HubPaths;

/// hub.toml 首版字段：配置结构版本；remote 等留待后续版本
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HubConfig {
    /// 配置结构版本
    pub schema_version: u32,
    /// 版本检查配置；缺这一段时整段取缺省值，已有 hub 不改配置也能跑
    #[serde(default)]
    pub update: UpdateConfig,
}

/// 版本检查的档位：被动 / 只手动 / 关闭
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    /// 被动检查加升级提示（缺省）
    #[default]
    Auto,
    /// 只有 `fsh doctor` 主动查
    Manual,
    /// 完全不查
    Off,
}

impl UpdateMode {
    /// 被动路径（每条命令末尾）查不查：只有 auto 查
    pub fn queries_passively(self) -> bool {
        self == Self::Auto
    }

    /// `fsh doctor` 查不查：只有明确关掉才不查——用户的选择不该被反复唠叨
    pub fn queries_in_doctor(self) -> bool {
        self != Self::Off
    }
}

/// hub.toml 的 `[update]` 表：版本检查的六项配置，全部带缺省值。
/// 缺哪一项就补这一项的缺省值，因此手写时只写想改的那几个键即可
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdateConfig {
    /// 检查档位
    pub mode: UpdateMode,
    /// 节流间隔小时数；`0` 表示每条命令都查
    pub interval_hours: u64,
    /// 发布源根地址
    pub registry: String,
    /// 哪个 dist-tag 算最新版
    pub channel: String,
    /// CI 环境里是否也提示
    pub notify_in_ci: bool,
    /// 输出被重定向（stderr 非 TTY）时是否也提示
    pub notify_when_piped: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            mode: UpdateMode::Auto,
            interval_hours: 24,
            registry: "https://registry.npmjs.org".to_string(),
            channel: "latest".to_string(),
            notify_in_ci: false,
            notify_when_piped: false,
        }
    }
}

impl HubConfig {
    /// 首版默认配置
    pub fn v1() -> Self {
        Self {
            schema_version: 1,
            update: UpdateConfig::default(),
        }
    }

    /// 序列化为 hub.toml 文本
    pub fn to_toml(&self) -> Result<String, HubError> {
        toml::to_string(self).map_err(HubError::ConfigTomlSer)
    }

    /// 从 hub.toml 文本解析
    pub fn from_toml(text: &str) -> Result<Self, HubError> {
        toml::from_str(text).map_err(HubError::ConfigTomlDe)
    }
}

/// 读取 hub.toml；文件缺失时返回 None（由调用方决定是否引导初始化）。
/// 读失败与解析失败在这里各记一次，上层不重复记
pub fn load_config(paths: &HubPaths) -> Result<Option<HubConfig>, HubError> {
    read_config(paths).inspect_err(|e| log_config_error(paths, e))
}

/// 配置读写的失败日志：归属边界自己决定什么时候记，消息只在这里成形一次
pub(crate) fn log_config_error(paths: &HubPaths, e: &HubError) {
    // 解析与读盘分开点名，便于按消息后缀筛
    let message = match e {
        HubError::ConfigTomlDe(_) => "解析配置失败",
        _ => "读取配置失败",
    };
    tracing::error!(path = %paths.hub_toml().display(), cause = %e, message);
}

/// 不记日志的配置读取：给需要自己决定日志口径的调用方用——
/// 被动版本检查要的是一条 WARN 而不是每个命令一条 ERROR
pub(crate) fn read_config(paths: &HubPaths) -> Result<Option<HubConfig>, HubError> {
    match fs::read_to_string(paths.hub_toml()) {
        Ok(text) => HubConfig::from_toml(&text).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
