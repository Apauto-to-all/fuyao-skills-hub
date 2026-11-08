//! 版本切换：把某技能在一次调用里的全部目标（可选的全局目标 + 零到多个项目目标）
//! 的已有登记记录切到指定版本或跟随最新。
//! 换靶用「旧入口改名占位 → 原路径建新入口 → 全部成功才删占位」：
//! 任一落点失败整批还原，登记簿最后一次性写；已是目标版本时零盘动作。

mod prepare;
mod run;

pub use run::upgrade;

use std::path::{Path, PathBuf};

use crate::link::LinkKind;
use crate::project::{Anchor, Destination};

/// upgrade 的公共预检结果：实跑与预演由此同源
pub(super) struct UpgradePlan {
    /// 换到的版本；None 表示跟随最新
    pub to_version: Option<String>,
    /// 新入口的源目录（固定版本是导出目录，跟随最新是工作副本）
    pub source: PathBuf,
    /// 固定版本的导出目录（跟随最新时为空）
    pub export_dir: Option<PathBuf>,
    /// 逐目标的预检结果，全局目标在最前
    pub targets: Vec<UpgradeTargetPlan>,
}

/// 一个目标的预检结果与执行结果
pub(super) struct UpgradeTargetPlan {
    /// 目标身份
    pub anchor: Anchor,
    /// 记录里的 agent 名单（版本切换不改名单）
    pub agents: Vec<String>,
    /// 切换前的登记版本
    pub from_version: Option<String>,
    /// 登记的链接方式：占位里的旧入口按它删
    pub kind: LinkKind,
    /// 该目标已处于目标版本：零盘动作
    pub idempotent: bool,
    /// 记录解析出的落点
    pub destinations: Vec<Destination>,
    /// 预演动作清单
    pub actions: Vec<String>,
    /// 幂等命中的提醒：拷贝档跟最新时盘上还是旧内容，拷贝不跟随源
    pub note: Option<String>,
    /// 换后实际采用的链接方式：预检时未知，建完入口才有值
    pub link: Option<LinkKind>,
}

/// 旧入口改名后的占位：同父目录下的 `.<技能名>.fsh-old`。
/// 带点前缀与固定后缀，不与技能目录名相撞
pub(super) fn placeholder_of(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let parent = dest.parent().unwrap_or(Path::new(""));
    parent.join(format!(".{name}.fsh-old"))
}
