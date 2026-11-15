//! 恢复：把归档目录里的键整目录移回 store；store 里同名键已占用即报冲突，
//! 不做静默改名。

use std::fs;
use std::path::{Path, PathBuf};

use super::outcome::{SkillOutcome, dry};
use super::{RestoreEnds, SkillError, skill_occupancy};
use crate::commit::{Face, Verb, message};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep, Steps};
use crate::write::{self, Applied, OnFailure};

/// 恢复：把归档目录里的键移回 store（store 里同名键已占用报 conflict），
/// 并自动提交。dry_run = true 时预演：只读不落盘、不取写锁。
pub fn restore(hub_root: &Path, key: &str, dry_run: bool) -> Result<SkillOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    write::run(
        hub_root,
        dry_run,
        // 目录移动失败时归档副本还在原处：盘上不留移了一半的 store 面
        OnFailure::Rollback,
        |paths| prepare_restore(paths, key),
        dry,
        |_ctx, plan| {
            fs::rename(&plan.from, &plan.to)?;
            Ok(Applied {
                outcome: SkillOutcome {
                    key: key.into(),
                    restored: true,
                    ..SkillOutcome::default()
                },
                milestones: (),
            })
        },
        |(), elapsed_ms| {
            // 恢复跨移动目录与机器提交两段外部操作：一条 INFO 交代键与整段耗时
            tracing::info!(key = %key, elapsed_ms = elapsed_ms, "skill 恢复完成");
        },
    )
}

/// 恢复预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct RestorePlan {
    /// 归档副本目录：移动的来源
    from: PathBuf,
    /// store 工作副本目录：移动的落点
    to: PathBuf,
    /// 将执行的步骤清单
    actions: Vec<PlanStep>,
}

impl Steps for RestorePlan {
    fn steps(&self) -> Vec<PlanStep> {
        self.actions.clone()
    }
}

/// 恢复预检：归档副本在场、store 侧空闲；动作清单在这里一次成形
fn prepare_restore(paths: &HubPaths, key: &str) -> Result<RestorePlan, SkillError> {
    // 两端一次判定：归档副本是来源，工作副本是落点且必须空闲
    let RestoreEnds { from, to } = skill_occupancy(paths, key).require_restorable()?;
    let actions = vec![
        PlanStep::between(
            ActionKind::Move,
            crate::naming::archive_subpath(key),
            crate::naming::store_subpath(key),
        )
        .targeting(key),
        PlanStep::commit(message(Verb::Restore, Face::Store, key)),
    ];
    Ok(RestorePlan { from, to, actions })
}
