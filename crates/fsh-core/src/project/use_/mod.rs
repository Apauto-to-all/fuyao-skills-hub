//! 使用：把技能铺给一次调用里的全部目标（可选的全局目标 + 零到多个项目目标），
//! 每个目标各登记一条技能记录。落点一律直连 store 工作副本。
//! 预检与实跑同源——预演只报告动作，实跑按同一份计划落盘。
//! 全部目标一起预检：任一目标不合格则整次不动盘；执行中途失败回滚本次已建落点。

mod prepare;
pub(super) mod run;

pub use run::use_skill;

use std::path::PathBuf;

use crate::link::LinkKind;
use crate::plan::PlanStep;
use crate::project::Anchor;
use crate::project::dest::Dest;

/// use 的公共预检结果：实跑与预演由此同源
struct UsePlan {
    /// 技能源目录（store 工作副本）
    source: PathBuf,
    /// 逐目标的预检结果，全局目标在最前
    targets: Vec<TargetPlan>,
}

/// 一个目标的预检结果与执行结果
struct TargetPlan {
    /// 目标身份
    anchor: Anchor,
    /// 去重后的 agent 键
    agents: Vec<String>,
    /// 去重后的落点
    destinations: Vec<Dest>,
    /// 预演动作清单
    actions: Vec<PlanStep>,
    /// 实际采用的链接方式：预检时未知，建完入口才有值
    link: Option<LinkKind>,
}
