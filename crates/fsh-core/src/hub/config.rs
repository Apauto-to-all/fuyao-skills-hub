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
}

impl HubConfig {
    /// 首版默认配置
    pub fn v1() -> Self {
        Self { schema_version: 1 }
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

/// 读取 hub.toml；文件缺失时返回 None（由调用方决定是否引导初始化）
pub fn load_config(paths: &HubPaths) -> Result<Option<HubConfig>, HubError> {
    match fs::read_to_string(paths.hub_toml()) {
        Ok(text) => HubConfig::from_toml(&text).map(Some).map_err(|e| {
            // 解析失败属配置不可读：在读出口记一次，上层不重复记
            tracing::error!(path = %paths.hub_toml().display(), cause = %e, "解析配置失败");
            e
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        // 读失败在归属它的边界记一次，上层不重复记
        Err(e) => {
            tracing::error!(path = %paths.hub_toml().display(), cause = %e, "读取配置失败");
            Err(e.into())
        }
    }
}
