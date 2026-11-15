//! 使用与撤回的结果载荷：一处落点、逐目标的使用结果、逐目标的撤回结果。
//! 全部字段都进 `--json` 信封，缺席项经 serde 跳过，预演信封与全部写命令同形。

use serde::{Deserialize, Serialize};

use crate::hub::Scope;
use crate::link::LinkKind;
use crate::plan::PlanStep;

/// 一处落点：多个 agent 共用同一实际目录时只出现一次
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    /// 实际落点绝对路径
    pub path: String,
    /// 解析到该落点的 agent 键
    pub agents: Vec<String>,
}

/// 信封形状只在这一处成形：工作值持 `PathBuf`，出口映射成字符串路径
impl From<super::dest::Dest> for Destination {
    fn from(dest: super::dest::Dest) -> Self {
        Self {
            path: dest.path.display().to_string(),
            agents: dest.agents,
        }
    }
}

/// 一个目标的使用结果：每个目标自带作用域、落点与链接方式
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetOutcome {
    /// 这个目标的作用域
    pub scope: Scope,
    /// 项目根绝对路径；全局目标没有项目，不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// 本次涉及的 agent（预演取计划里的名单，实跑取实际登记的名单）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub agents: Vec<String>,
    /// 实际采用的链接方式（仅实跑出现；作用于该目标全部落点）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkKind>,
    /// 落点：预演列计划落点，实跑列实际建立的落点；两种形态都填
    #[serde(skip_serializing_if = "crate::is_default")]
    pub destinations: Vec<Destination>,
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}

/// use 的结果载荷：实跑直出各目标的落点与登记，预演直出各目标的动作清单
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 命令级步骤（仅预演出现）：不属于任何单个目标的那几步，如动盘前的手改快照
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
    /// 逐个目标的结果，全局目标在最前（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub targets: Vec<TargetOutcome>,
}

/// unuse 的结果载荷：实跑直出各目标删掉的落点，预演直出各目标的动作清单
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnuseOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 命令级步骤（仅预演出现）：不属于任何单个目标的那几步，如动盘前的手改快照
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
    /// 逐个目标的结果，全局目标在最前
    #[serde(skip_serializing_if = "crate::is_default")]
    pub targets: Vec<UnuseTarget>,
}

/// 一个目标的撤回结果：作用域、项目、登记 agent 与删掉的落点
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}
