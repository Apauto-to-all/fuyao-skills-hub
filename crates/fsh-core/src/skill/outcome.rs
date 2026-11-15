//! skill 写命令的结果信封：add / archive / remove / restore 四个动词共用。
//! 预演直出 `{dry_run, actions}`，实跑直出 `{key}`；缺席字段经 serde 跳过，
//! 两种信封共用同一形状。完成的动词只给文本模式的完成陈述取名，不入信封。

use serde::{Deserialize, Serialize};

use crate::commit::Verb;
use crate::plan::{Plan, PlanStep};

/// skill 写命令的结果载荷。实跑只填业务字段（键与动词），预演只填动作清单；
/// 缺席字段不入信封
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 实跑完成的动词（仅实跑出现）：完成陈述的措辞取自这里，不入信封
    #[serde(skip)]
    pub verb: Option<Verb>,
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}

/// 预演载荷：预演标志置真，动作清单即本次将要执行的完整步骤——
/// 骨架在计划之前先做的那几步（手改快照）在前，计划自身的步骤在后
pub(super) fn dry(plan: Plan<()>, command: Vec<PlanStep>) -> SkillOutcome {
    let mut actions = command;
    actions.extend(plan.describe());
    SkillOutcome {
        dry_run: true,
        actions,
        ..SkillOutcome::default()
    }
}
