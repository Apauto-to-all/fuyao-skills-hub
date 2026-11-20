//! 版本检查配置的读取：被动路径上的容错读取。
//! `hub.toml` 写坏时按缺省值继续、只记一条 WARN——一个手误不该让所有命令失败。

use crate::hub::{self, UpdateConfig};
use crate::paths::HubPaths;

/// 读 `[update]` 表；文件缺失、解析失败、读失败一律回退缺省值。
/// 容错的判据是「这条功能能不能继续」而不是「配置对不对」：真问题由
/// `fsh doctor` 的装配完整性检查报成 FAIL，不会在这里被吞掉。
/// 走不记日志的读法，好让整条被动路径上只留这一条 WARN
pub fn load_update_config(paths: &HubPaths) -> UpdateConfig {
    match hub::read_config(paths) {
        Ok(Some(cfg)) => cfg.update,
        // 未初始化的 hub：被动路径随后按「hub 未初始化」直接退出，配置取值无所谓
        Ok(None) => UpdateConfig::default(),
        Err(e) => {
            tracing::warn!(
                path = %paths.hub_toml().display(),
                cause = %e,
                "版本检查配置降级"
            );
            UpdateConfig::default()
        }
    }
}
