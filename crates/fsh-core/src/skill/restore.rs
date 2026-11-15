//! 恢复：把归档目录里的键整目录移回 store；store 里同名键已占用即报冲突，
//! 不做静默改名。

use std::path::Path;

use super::outcome::{SkillOutcome, dry};
use super::{RestoreEnds, SkillError, skill_occupancy};
use crate::commit::{Face, Verb, message};
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
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
        |_ctx, _plan| {
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

/// 恢复预检：归档副本在场、store 侧空闲；计划在这里一次成形
fn prepare_restore(paths: &HubPaths, key: &str) -> Result<Plan<()>, SkillError> {
    // 两端一次判定：归档副本是来源，工作副本是落点且必须空闲
    let RestoreEnds { from, to } = skill_occupancy(paths, key).require_restorable()?;
    let mut plan = Plan::new(());
    plan.push(
        PlanStep::between(
            ActionKind::Move,
            naming::archive_subpath(key),
            naming::store_subpath(key),
        )
        .targeting(key),
        Act::Move { from, to },
    );
    plan.push(
        PlanStep::commit(message(Verb::Restore, Face::Store, key)),
        Act::None,
    );
    Ok(plan)
}
