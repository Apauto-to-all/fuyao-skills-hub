//! hub.toml：提交身份、GC 策略与备份提醒的类型与读写。
//! 配置是 tracked 的版本真身之一，读写都走这里。

use std::fs;

use serde::{Deserialize, Serialize};

use super::HubError;
use crate::paths::HubPaths;

/// 提交身份（写入每次 commit）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserIdentity {
    /// 提交者名
    pub name: String,
    /// 提交者邮箱
    pub email: String,
}

impl Default for UserIdentity {
    fn default() -> Self {
        Self {
            name: "fuyao-skills-hub".to_string(),
            email: "fuyao-skills-hub@local".to_string(),
        }
    }
}

/// GC 策略配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GcConfig {
    /// 导出目录回收宽限天数（0 = 即时清）
    // 宽限窗口只对带时间戳的对象生效（登记簿死项目记录）；
    // 导出目录无「失引用时刻」可考，无引用即清，后悔药 = 按 tag 重新导出
    #[serde(default)]
    pub grace_days: u32,
}

/// 备份提醒配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupConfig {
    /// 超过 N 天未备份时 doctor 提醒（N 天 = 本值）
    #[serde(default = "default_remind_days")]
    pub remind_days: u32,
}

/// remind_days 缺省值（doctor 兜底读取配置失败时也用它，保持单一来源）
pub(crate) fn default_remind_days() -> u32 {
    7
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            remind_days: default_remind_days(),
        }
    }
}

/// hub.toml 首版字段：身份 + GC 策略 + 备份提醒；remote 等留待后续版本
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HubConfig {
    /// 配置结构版本
    pub schema_version: u32,
    /// 提交身份
    #[serde(default)]
    pub user: UserIdentity,
    /// GC 策略
    #[serde(default)]
    pub gc: GcConfig,
    /// 备份提醒
    #[serde(default)]
    pub backup: BackupConfig,
}

impl HubConfig {
    /// 首版默认配置
    pub fn v1() -> Self {
        Self {
            schema_version: 1,
            user: UserIdentity::default(),
            gc: GcConfig::default(),
            backup: BackupConfig::default(),
        }
    }

    /// 序列化为 hub.toml 文本
    pub fn to_toml(&self) -> Result<String, HubError> {
        Ok(toml::to_string(self)?)
    }

    /// 从 hub.toml 文本解析
    pub fn from_toml(text: &str) -> Result<Self, HubError> {
        Ok(toml::from_str(text)?)
    }
}

/// 读取 hub.toml；文件缺失时返回 None（由调用方决定是否引导初始化）
pub fn load_config(paths: &HubPaths) -> Result<Option<HubConfig>, HubError> {
    match fs::read_to_string(paths.hub_toml()) {
        Ok(text) => Ok(Some(HubConfig::from_toml(&text)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 把配置写回 hub.toml
pub fn save_config(paths: &HubPaths, cfg: &HubConfig) -> Result<(), HubError> {
    fs::write(paths.hub_toml(), cfg.to_toml()?)?;
    Ok(())
}
