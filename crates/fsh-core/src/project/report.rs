//! 使用、撤回与版本切换的结果载荷：一处落点、逐目标的使用结果、逐目标的撤回结果、
//! 逐目标的版本切换结果。
//! 全部字段都进 `--json` 信封，缺席项经 serde 跳过，预演信封与全部写命令同形。

use serde::{Deserialize, Serialize};

use crate::hub::Scope;
use crate::link::LinkKind;

/// 一处落点：多个 agent 共用同一实际目录时只出现一次
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    /// 实际落点绝对路径
    pub path: String,
    /// 解析到该落点的 agent 键
    pub agents: Vec<String>,
}

/// 一个目标的使用结果：每个目标自带作用域、落点与链接方式
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetOutcome {
    /// 这个目标的作用域
    pub scope: Scope,
    /// 项目根绝对路径；全局目标没有项目，不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// 本次登记的全部 agent（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub agents: Vec<String>,
    /// 实际采用的链接方式（仅实跑出现；作用于该目标全部落点）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkKind>,
    /// 实际建立的落点（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub destinations: Vec<Destination>,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}

/// use 的结果载荷：实跑直出各目标的落点与登记，预演直出各目标的动作清单
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 固定版本；null 表示跟随最新
    pub version: Option<String>,
    /// 逐个目标的结果，全局目标在最前（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub targets: Vec<TargetOutcome>,
    /// 使用后顺手回收的导出目录统计；没有发生回收时为空
    pub cleanup: Option<CleanupReport>,
}

/// 使用后顺手回收的统计
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupReport {
    /// 处理的无引用导出目录条目数
    pub removed: usize,
    /// 回收释放的字节数
    pub freed_bytes: u64,
}

/// upgrade 的结果载荷：实跑直出各目标换成的落点与登记变更，预演直出各目标的动作清单
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpgradeOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 换到的版本；null 表示跟随最新
    pub version: Option<String>,
    /// 逐个目标的结果，全局目标在最前
    #[serde(skip_serializing_if = "crate::is_default")]
    pub targets: Vec<UpgradeTarget>,
    /// 换完顺手回收的导出目录统计；没有发生回收时为空
    pub cleanup: Option<CleanupReport>,
}

/// 一个目标的版本切换结果：切换前版本、是否幂等命中、换成后的落点与链接方式
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpgradeTarget {
    /// 这个目标的作用域
    pub scope: Scope,
    /// 项目根绝对路径；全局目标没有项目，不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// 这条记录服务的 agent（版本切换不改名单，原样带出）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub agents: Vec<String>,
    /// 切换前的登记版本；null 表示原来就跟最新
    pub from: Option<String>,
    /// 该目标已处于目标版本：零盘动作（仅这种目标出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub idempotent: bool,
    /// 登记里的链接方式（仅实跑出现；幂等命中时就是未变动的登记值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkKind>,
    /// 记录解析出的落点（仅实跑出现；幂等命中时就是未变动的旧落点）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub destinations: Vec<Destination>,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
    /// 幂等命中的提醒（仅拷贝档跟最新时出现）：拷贝是快照，不跟随源
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// unuse 的结果载荷：实跑直出各目标删掉的落点，预演直出各目标的动作清单
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnuseOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 逐个目标的结果，全局目标在最前
    #[serde(skip_serializing_if = "crate::is_default")]
    pub targets: Vec<UnuseTarget>,
}

/// 一个目标的撤回结果：作用域、项目、登记 agent 与删掉的落点
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnuseTarget {
    /// 这个目标的作用域
    pub scope: Scope,
    /// 项目根绝对路径；全局目标没有项目，不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// 本次撤走的 agent（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub agents: Vec<String>,
    /// 该记录登记的链接方式（仅实跑出现）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkKind>,
    /// 实际删除的落点（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub removed: Vec<String>,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}
