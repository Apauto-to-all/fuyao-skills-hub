//! skill 写命令的结果信封：add / archive / remove / restore 四个动词共用。
//! 预演直出 `{dry_run, actions}`，实跑直出 `{key}` 与该动词的完成旗标；
//! 缺席字段经 serde 跳过，两种信封共用同一形状。

use serde::{Deserialize, Serialize};

use crate::plan::PlanStep;

/// skill 写命令的结果载荷。实跑只填业务字段（键与对应动词的完成旗标），
/// 预演只填动作清单；缺席字段不入信封
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 归档完成标志（仅 archive 实跑出现，恒为 true）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub archived: bool,
    /// 移除完成标志（仅 remove 实跑出现，恒为 true）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub removed: bool,
    /// 恢复完成标志（仅 restore 实跑出现，恒为 true）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub restored: bool,
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}

/// 预演载荷：预演标志置真，动作清单即本次将要执行的完整步骤。
/// 计划本体在 add / archive / remove / restore 只贡献步骤，业务字段为空
pub(super) fn dry(dry: crate::write::DryPlan<impl Sized>) -> SkillOutcome {
    SkillOutcome {
        dry_run: true,
        actions: dry.all_steps(),
        ..SkillOutcome::default()
    }
}
