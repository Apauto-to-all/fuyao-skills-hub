//! 一次读盘：登记簿、内置表、两面目录、登记落点各取一次，盘面各项检查共用这一份。

use crate::bundled::{self, BuiltinSkill};
use crate::hub::{self, HubConfig, ProjectRegistry, UpdateConfig};
use crate::paths::HubPaths;
use crate::project::{Dest, resolve_destinations};
use crate::skill::{FoundSkill, walk_face};

/// 一个面走过一遍的结果：面名（报告里的键前缀）与面内全部目录
pub(super) struct FaceScan {
    /// 面名：`store` 或 `archive`
    pub(super) name: &'static str,
    /// 面内走到的全部目录，父先子后
    pub(super) skills: Vec<FoundSkill>,
}

/// 一次读盘的全部事实：盘面各项检查只看这一份，不各自再从旁路取输入
pub(super) struct Scan {
    /// hub 路径族
    pub(super) paths: HubPaths,
    /// 内置技能表（编译期内嵌，只取一次）
    pub(super) builtin: Vec<BuiltinSkill>,
    /// 登记簿：只解析一次，读到坏文件的两项共用同一句原因
    pub(super) registry: ProjectRegistry,
    /// 登记簿读不出的原因；None 表示读到了
    pub(super) registry_error: Option<String>,
    /// 登记在案的落点：路径与 agent 归属一起带出；Err 是核对不了的原因
    pub(super) destinations: Result<Vec<Dest>, String>,
    /// hub.toml：只解析一次，装配检查与版本检查共用这一份结论
    pub(super) config: Result<Option<HubConfig>, String>,
    /// store 与 archive 两面，各走一遍
    pub(super) faces: Vec<FaceScan>,
}

impl Scan {
    /// 版本检查用的配置：读不出配置就无从判定档位，报原因而不是退回缺省值
    /// 去联网——那会在用户配置写坏时替他去猜一个发布源
    pub(super) fn update_config(&self) -> Result<UpdateConfig, String> {
        match &self.config {
            Ok(Some(cfg)) => Ok(cfg.update.clone()),
            Ok(None) => Ok(UpdateConfig::default()),
            Err(error) => Err(error.clone()),
        }
    }
}

/// 读一次盘：登记簿与 agent 清单各读一次，落点由这一份登记簿解析，
/// store 与 archive 各走一遍目录树
pub(super) fn scan(paths: HubPaths) -> Scan {
    let (registry, registry_error) = read_registry(&paths);
    // 登记簿读不出就无从解析落点：原因与登记簿那一项同源，不另说一套
    let destinations = match &registry_error {
        Some(error) => Err(error.clone()),
        None => resolve_destinations(&paths, &registry).map_err(|e| e.to_string()),
    };
    let faces = [(paths.store(), "store"), (paths.archive(), "archive")]
        .into_iter()
        .map(|(dir, name)| FaceScan {
            name,
            skills: walk_face(&dir).collect(),
        })
        .collect();
    // 配置走不记日志的读法：坏配置在这里记一条 ERROR，装配检查报成 FAIL 并点名，
    // 版本检查复用同一份结论——同一个事件只有一条日志、一处说明
    let config = match paths.hub_toml().is_file() {
        true => hub::read_config(&paths).map_err(|e| {
            hub::log_config_error(&paths, &e);
            e.to_string()
        }),
        false => Ok(None),
    };
    Scan {
        builtin: bundled::skills(),
        registry,
        registry_error,
        destinations,
        config,
        faces,
        paths,
    }
}

/// 读一次登记簿：读不出的原因在这里转成文本带出，不在上层重记
fn read_registry(paths: &HubPaths) -> (ProjectRegistry, Option<String>) {
    match hub::load_registry(paths) {
        Ok(registry) => (registry, None),
        Err(error) => (ProjectRegistry::default(), Some(error.to_string())),
    }
}
