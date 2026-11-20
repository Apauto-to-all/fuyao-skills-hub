//! 恢复：把归档目录里的技能整目录移回 store（`archive\<键>\` → `store\<键>\`）。
//! 一次恢复一整套：工作副本面已有同键的技能跳过并报告（不静默改名），
//! 其余照常恢复，整批只留一条自动提交。

use std::path::Path;

use super::outcome::{SelectionOutcome, Skipped};
use super::{RestoreEnds, SkillError, StateError, skill_occupancy};
use crate::commit::{Face, Verb, batch_message};
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::select::{self, Selector};
use crate::write::{self, Applied, OnFailure};

/// 一次恢复的业务数据：真正做掉的键与被跳过的键
#[derive(Debug, Default)]
struct RestoreSet {
    /// 本次做掉的键
    applied: Vec<String>,
    /// 预期内不适用而跳过的键
    skipped: Vec<Skipped>,
}

/// 恢复一整套：按选择器从归档面解析出键，工作副本面已有同键的跳过并报告，
/// 其余整目录移回 store，最后一条机器提交。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn restore(
    hub_root: &Path,
    selector: &Selector,
    dry_run: bool,
) -> Result<SelectionOutcome, SkillError> {
    // 键形状先判：写错的前缀与技能名当场点名，不落到盘面判定
    selector.check()?;
    write::run(
        hub_root,
        dry_run,
        // 目录移动失败时归档副本还在原处：盘上不留移了一半的工作副本面
        OnFailure::Rollback,
        |paths| prepare_restore(paths, selector),
        |plan, command| {
            let mut actions = command;
            actions.extend(plan.describe());
            let RestoreSet { applied, skipped } = plan.data;
            SelectionOutcome {
                dry_run: true,
                applied,
                skipped,
                actions,
                ..SelectionOutcome::default()
            }
        },
        |_ctx, plan| {
            let RestoreSet { applied, skipped } = plan.data;
            let count = applied.len();
            Ok(Applied {
                outcome: SelectionOutcome {
                    applied,
                    skipped,
                    verb: Some(Verb::Restore),
                    ..SelectionOutcome::default()
                },
                milestones: count,
            })
        },
        |count, elapsed_ms| {
            // 恢复跨移动目录与机器提交两段外部操作：一条 INFO 交代整套、条数与整段耗时
            tracing::info!(
                prefix = %selector.label(),
                count,
                elapsed_ms = elapsed_ms,
                "skill 恢复完成"
            );
        },
    )
}

/// 恢复预检：解析选择 → 逐个确认归档副本在场、工作副本落点空闲。
/// 计划在这里一次成形：工作副本面已有同键的进跳过清单，其余键进移动步骤；
/// 整批一条提交，一条都没做成就没有提交步
fn prepare_restore(paths: &HubPaths, selector: &Selector) -> Result<Plan<RestoreSet>, SkillError> {
    // 整套的候选：归档面里扫到的技能，按段边界归到前缀之下
    let available = select::members(
        super::walk_face(&paths.archive())
            .filter(|skill| skill.has_skill_md)
            .map(|skill| skill.key),
        selector.prefix(),
    );
    let keys = selector.resolve(&available)?;
    let mut plan = Plan::new(RestoreSet::default());
    for key in &keys {
        let occupancy = skill_occupancy(paths, key);
        // 两端一次判定：归档副本是来源，工作副本是落点且必须空闲
        let RestoreEnds { from, to } = match occupancy.require_restorable() {
            Ok(ends) => ends,
            // 工作副本面已有同键：预期内的不适用，跳过并报告，不静默改名
            Err(StateError::Conflict { .. }) => {
                plan.data
                    .skipped
                    .push(Skipped::of(key, &SkillError::Conflict { key: key.clone() }));
                continue;
            }
            // 归档副本不在场：候选扫自归档面，只有盘面在计划前变了才走到这里
            Err(e) => return Err(e.into()),
        };
        plan.data.applied.push(key.clone());
        plan.push(
            PlanStep::between(
                ActionKind::Move,
                naming::archive_subpath(key),
                naming::store_subpath(key),
            )
            .targeting(key),
            Act::Move { from, to },
        );
    }
    if !plan.data.applied.is_empty() {
        let commit = batch_message(
            Verb::Restore,
            Face::Store,
            selector.label(),
            plan.data.applied.len(),
        );
        plan.push(PlanStep::commit(commit), Act::None);
    }
    Ok(plan)
}
