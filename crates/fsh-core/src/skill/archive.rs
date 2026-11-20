//! 归档：把技能的工作副本整目录移入 hub 平级的归档目录（archive\\<键>\\，
//! 与 store 同构、tracked）。一次归档一整套：被引用的技能跳过并报告，其余
//! 照常归档，整批只留一条自动提交。

use std::path::Path;

use super::outcome::{SelectionOutcome, Skipped};
use super::{SkillError, skill_occupancy, unreferenced};
use crate::commit::{Face, Verb, batch_message};
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::select::{self, Selector};
use crate::write::{self, Applied, OnFailure};

/// 一次归档的业务数据：真正做掉的键与被跳过的键
#[derive(Debug, Default)]
struct ArchiveSet {
    /// 本次做掉的键
    applied: Vec<String>,
    /// 预期内不适用而跳过的键
    skipped: Vec<Skipped>,
}

/// 归档一整套：按选择器解析出键，被引用的跳过并报告，其余整目录移入归档面
/// （目标键已占用报 conflict，整批不入），最后一条机器提交。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn archive(
    hub_root: &Path,
    selector: &Selector,
    dry_run: bool,
) -> Result<SelectionOutcome, SkillError> {
    // 键形状先判：写错的前缀与技能名当场点名，不落到盘面判定
    selector.check()?;
    write::run(
        hub_root,
        dry_run,
        // 目录移动失败时工作副本还在原处：盘上不留移了一半的归档面
        OnFailure::Rollback,
        |paths| prepare_archive(paths, selector),
        |plan, command| {
            let mut actions = command;
            actions.extend(plan.describe());
            let ArchiveSet { applied, skipped } = plan.data;
            SelectionOutcome {
                dry_run: true,
                applied,
                skipped,
                actions,
                ..SelectionOutcome::default()
            }
        },
        |_ctx, plan| {
            let ArchiveSet { applied, skipped } = plan.data;
            let count = applied.len();
            Ok(Applied {
                outcome: SelectionOutcome {
                    applied,
                    skipped,
                    verb: Some(Verb::Archive),
                    ..SelectionOutcome::default()
                },
                milestones: count,
            })
        },
        |count, elapsed_ms| {
            // 归档跨移动目录与机器提交两段外部操作：一条 INFO 交代整套、条数与整段耗时
            tracing::info!(
                prefix = %selector.label(),
                count,
                elapsed_ms = elapsed_ms,
                "skill 归档完成"
            );
        },
    )
}

/// 归档预检：解析选择 → 逐个确认来源活跃、无人引用、归档侧空闲。
/// 计划在这里一次成形：被引用的键进跳过清单，其余键进移动步骤；
/// 整批一条提交，一条都没做成就没有提交步
fn prepare_archive(paths: &HubPaths, selector: &Selector) -> Result<Plan<ArchiveSet>, SkillError> {
    // 整套的候选：这个面里扫到的技能，按段边界归到前缀之下
    let available = select::members(
        super::walk_face(&paths.store())
            .filter(|skill| skill.has_skill_md)
            .map(|skill| skill.key),
        selector.prefix(),
    );
    let keys = selector.resolve(&available)?;
    // 引用判定只读一次登记簿：整批共用同一份账
    let registry = crate::hub::load_registry(paths)?;
    let mut plan = Plan::new(ArchiveSet::default());
    for key in &keys {
        let occupancy = skill_occupancy(paths, key);
        // 来源：工作副本必须在场，缺席按不存在或已归档分别拒绝
        let from = occupancy.require_active()?;
        // 归档前必须无人引用：预期内的不适用，跳过并报告，不堵住其余技能
        if let Err(e) = unreferenced(&registry, key) {
            plan.data.skipped.push(Skipped::of(key, &e));
            continue;
        }
        // 落点：归档侧必须空闲，同名键已占用统一 conflict，不做静默改名
        let to = occupancy.require_free_archive()?;
        plan.data.applied.push(key.clone());
        plan.push(
            PlanStep::between(
                ActionKind::Move,
                naming::store_subpath(key),
                naming::archive_subpath(key),
            )
            .targeting(key),
            Act::Move { from, to },
        );
    }
    if !plan.data.applied.is_empty() {
        let commit = batch_message(Verb::Archive, &[Face::Store], &plan.data.applied);
        plan.push(PlanStep::commit(commit), Act::None);
    }
    Ok(plan)
}
