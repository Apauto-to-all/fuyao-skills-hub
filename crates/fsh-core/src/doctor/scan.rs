//! 一次读盘：登记簿、内置表、两面目录、登记落点各取一次，十项检查共用这一份。

use crate::bundled::{self, BuiltinSkill};
use crate::hub::{self, ProjectRegistry};
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

/// 登记簿的一次读取结果：读不出时带原因、用空登记簿继续
pub(super) struct RegistryRead {
    /// 读到的登记簿（读不出时是空登记簿）
    pub(super) registry: ProjectRegistry,
    /// 读不出的原因
    pub(super) error: Option<String>,
}

/// 一次读盘的全部事实：十项检查只看这一份，不各自再从旁路取输入
pub(super) struct Scan {
    /// hub 路径族
    pub(super) paths: HubPaths,
    /// 内置技能表（编译期内嵌，只取一次）
    pub(super) builtin: Vec<BuiltinSkill>,
    /// 登记簿：只解析一次，读到坏文件的两项共用同一句原因
    pub(super) registry: RegistryRead,
    /// 登记在案的落点：路径与 agent 归属一起带出；Err 是核对不了的原因
    pub(super) destinations: Result<Vec<Dest>, String>,
    /// store 与 archive 两面，各走一遍
    pub(super) faces: Vec<FaceScan>,
}

/// 读一次盘：登记簿与 agent 清单各读一次，落点由这一份登记簿解析，
/// store 与 archive 各走一遍目录树
pub(super) fn scan(paths: HubPaths) -> Scan {
    let registry = read_registry(&paths);
    // 登记簿读不出就无从解析落点：原因与登记簿那一项同源，不另说一套
    let destinations = match &registry.error {
        Some(error) => Err(error.clone()),
        None => resolve_destinations(&paths, &registry.registry).map_err(|e| e.to_string()),
    };
    let faces = [(paths.store(), "store"), (paths.archive(), "archive")]
        .into_iter()
        .map(|(dir, name)| FaceScan {
            name,
            skills: walk_face(&dir).collect(),
        })
        .collect();
    Scan {
        builtin: bundled::skills(),
        registry,
        destinations,
        faces,
        paths,
    }
}

/// 读一次登记簿：读不出的原因在这里转成文本带出，不在上层重记
fn read_registry(paths: &HubPaths) -> RegistryRead {
    match hub::load_registry(paths) {
        Ok(registry) => RegistryRead {
            registry,
            error: None,
        },
        Err(error) => RegistryRead {
            registry: ProjectRegistry::default(),
            error: Some(error.to_string()),
        },
    }
}
