//! skill 写命令的结果载荷。单键动词直出 `{key}`，选择器动词直出
//! `{applied[], skipped[]}` 两个清单；预演另带动作清单。缺席字段经 serde
//! 跳过，预演与实跑共用同一形状。完成的动词只给文本模式的完成陈述取名，不入信封。

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::ErrorCode;
use crate::commit::Verb;
use crate::plan::{Plan, PlanStep};

/// 单键写命令的结果载荷。实跑只填业务字段（键与动词），预演只填动作清单；
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

/// 选择器写命令的结果载荷：做掉的键与被跳过的项分列。两个清单恒出现
/// （哪怕为空），脚本据此判断「这次其实什么都没做」；`skipped` 只在整批至少
/// 做成一条时才不算失败，退出码规则见 [`SelectionOutcome::code`]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SelectionOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 本次做掉的键，按选择顺序
    pub applied: Vec<String>,
    /// 预期内不适用而跳过的键，带稳定码与消息
    pub skipped: Vec<Skipped>,
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
    /// 实跑完成的动词（仅实跑出现）：完成陈述的措辞取自这里，不入信封
    #[serde(skip)]
    pub verb: Option<Verb>,
}

impl SelectionOutcome {
    /// 退出码：一条都没做成、却有被跳过的项 → 1（这次其实什么都没做）；否则 0
    pub fn code(&self) -> i32 {
        i32::from(self.applied.is_empty() && !self.skipped.is_empty())
    }
}

/// 跳过的一项：键、稳定码与消息。预期内的「不适用」走这里，不算失败
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    /// 被跳过的技能键
    pub key: String,
    /// 稳定码，与失败信封同一套
    pub code: &'static str,
    /// 人类可读的原因
    pub message: String,
}

impl Skipped {
    /// 从错误取稳定码与消息：跳过与失败说的是同一句话
    pub fn of(key: &str, e: &(impl ErrorCode + Display)) -> Self {
        Self {
            key: key.to_string(),
            code: e.error_code(),
            message: e.to_string(),
        }
    }
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
