//! 使用：首次把某未归档技能按版本铺给一次调用里的全部目标
//! （可选的全局目标 + 零到多个项目目标），每个目标各登记一条技能记录。
//! 预检与实跑同源——预演只报告动作，实跑按同一份计划落盘。
//! 全部目标一起预检：任一目标不合格则整次不动盘；执行中途失败回滚本次已建落点。

mod prepare;
mod run;

pub use run::use_skill;

use std::path::PathBuf;

use crate::hub::Scope;
use crate::identity::display_path;
use crate::link::LinkKind;
use crate::project::Destination;

/// 一个目标的身份：全局作用域，或某个项目的绝对路径
#[derive(Debug, Clone, PartialEq, Eq)]
enum Anchor {
    /// 全局技能目录，一台机器一份
    Global,
    /// 项目根（规范化绝对路径）
    Project(PathBuf),
}

impl Anchor {
    /// 作用域：登记记录与 JSON 里的 scope
    fn scope(&self) -> Scope {
        match self {
            Anchor::Global => Scope::Global,
            Anchor::Project(_) => Scope::Project,
        }
    }

    /// 项目根显示路径；全局目标没有项目
    fn project(&self) -> Option<String> {
        match self {
            Anchor::Global => None,
            Anchor::Project(dir) => Some(display_path(dir)),
        }
    }

    /// 日志标识：项目记绝对路径，全局记 global
    fn log_id(&self) -> String {
        match self {
            Anchor::Global => "global".to_string(),
            Anchor::Project(dir) => display_path(dir),
        }
    }
}

/// use 的公共预检结果：实跑与预演由此同源
struct UsePlan {
    /// 技能源目录（工作副本或导出目录）
    source: PathBuf,
    /// 固定版本
    version: Option<String>,
    /// 固定版本的导出目录（跟随最新时为空）
    export_dir: Option<PathBuf>,
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
    destinations: Vec<Destination>,
    /// 预演动作清单
    actions: Vec<String>,
    /// 实际采用的链接方式：预检时未知，建完入口才有值
    link: Option<LinkKind>,
}
