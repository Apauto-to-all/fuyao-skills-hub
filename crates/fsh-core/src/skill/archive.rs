//! 归档：把技能的工作副本整目录移入 hub 平级的归档目录（archive\<键>\，
//! 与 store 同构、tracked）；技能仍被引用时拒绝。

use std::path::Path;

use super::outcome::{SkillOutcome, dry};
use super::{SkillError, ensure_unreferenced, skill_occupancy};
use crate::commit::{Face, Verb, message};
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::write::{self, Applied, OnFailure};

/// 归档：先查登记簿，被项目引用即拒绝；通过后把工作副本整目录移入
/// 归档目录（目标键已占用报 conflict），并自动提交。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn archive(hub_root: &Path, key: &str, dry_run: bool) -> Result<SkillOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    write::run(
        hub_root,
        dry_run,
        // 目录移动失败时工作副本还在原处：盘上不留移了一半的归档面
        OnFailure::Rollback,
        |paths| prepare_archive(paths, key),
        dry,
        |_ctx, _plan| {
            Ok(Applied {
                outcome: SkillOutcome {
                    key: key.into(),
                    archived: true,
                    ..SkillOutcome::default()
                },
                milestones: (),
            })
        },
        |(), elapsed_ms| {
            // 归档跨移动目录与机器提交两段外部操作：一条 INFO 交代键与整段耗时
            tracing::info!(key = %key, elapsed_ms = elapsed_ms, "skill 归档完成");
        },
    )
}

/// 归档预检：确认来源活跃、归档侧空闲、无人引用；计划在这里一次成形
fn prepare_archive(paths: &HubPaths, key: &str) -> Result<Plan<()>, SkillError> {
    let occupancy = skill_occupancy(paths, key);
    // 来源：工作副本必须在场，缺席按不存在或已归档分别拒绝
    let from = occupancy.require_active()?;
    // 归档前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    // 落点：归档侧必须空闲，同名键已占用统一 conflict，不做静默改名
    let to = occupancy.require_free_archive()?;
    let mut plan = Plan::new(());
    plan.push(
        PlanStep::between(
            ActionKind::Move,
            naming::store_subpath(key),
            naming::archive_subpath(key),
        )
        .targeting(key),
        Act::Move { from, to },
    );
    plan.push(
        PlanStep::commit(message(Verb::Archive, Face::Store, key)),
        Act::None,
    );
    Ok(plan)
}
