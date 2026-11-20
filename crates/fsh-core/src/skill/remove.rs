//! 彻底删除：两侧副本（store 工作副本与 archive 归档副本）哪侧在场删哪侧，
//! 被项目引用即跳过并报告，无强制开关。一次删除一整套：被引用的技能跳过，
//! 其余照常删，整批只留一条自动提交。

use std::path::Path;

use super::outcome::{KeySet, SelectionOutcome, Skipped, dry};
use super::{SkillError, skill_occupancy, unreferenced};
use crate::commit::{Face, Verb, batch_message};
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::select::{self, Selector};
use crate::write::{self, Applied, OnFailure};

/// 彻底删除：先查登记簿（被引用即跳过并报告），再删在场的副本——两侧都在就
/// 两侧一起删；历史 commit 留在审计日志里。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn remove(
    hub_root: &Path,
    selector: &Selector,
    dry_run: bool,
) -> Result<SelectionOutcome, SkillError> {
    // 键形状先判：写错的前缀与技能名当场点名，不落到「技能不存在」
    selector.check()?;
    write::run(
        hub_root,
        dry_run,
        // 删除不回滚：删到一半失败时已删的部分保持删掉，重跑即可续完
        OnFailure::Resume,
        |paths| prepare_remove(paths, selector),
        dry,
        |_ctx, plan| {
            let KeySet { applied, skipped } = plan.data;
            let count = applied.len();
            Ok(Applied {
                outcome: SelectionOutcome {
                    applied,
                    skipped,
                    verb: Some(Verb::Remove),
                    ..SelectionOutcome::default()
                },
                milestones: count,
            })
        },
        |count, elapsed_ms| {
            // 移除不可逆：整套名、条数与整段耗时一并落 INFO，事后可核对删了什么
            tracing::info!(
                prefix = %selector.label(),
                count,
                elapsed_ms = elapsed_ms,
                "skill 移除完成"
            );
        },
    )
}

/// remove 预检：解析选择 → 逐个确认至少一侧副本在场、无人引用。
/// 两侧都是删除对象，任一面在场的技能都算这套里的候选；被引用的键进跳过清单，
/// 其余键按在场的面各排一步删除，整批一条提交，一条都没做成就没有提交步
fn prepare_remove(paths: &HubPaths, selector: &Selector) -> Result<Plan<KeySet>, SkillError> {
    // 整套的候选：两个面里扫到的技能并成一份键清单，同一键在两面都有只算一条
    let mut available: Vec<String> = super::walk_face(&paths.store())
        .chain(super::walk_face(&paths.archive()))
        .filter(|skill| skill.has_skill_md)
        .map(|skill| skill.key)
        .collect();
    available.sort();
    available.dedup();
    let keys = selector.resolve(&select::members(available, selector.prefix()))?;
    // 引用判定只读一次登记簿：整批共用同一份账
    let registry = crate::hub::load_registry(paths)?;
    // 提交消息的目标面：只点名这一批真正删到的面，按 store → archive 的固定顺序
    let mut faces: Vec<Face> = Vec::new();
    let mut plan = Plan::new(KeySet::default());
    for key in &keys {
        let occupancy = skill_occupancy(paths, key);
        // 至少一侧在场：两侧皆无报 skill_not_found
        occupancy.require_removable()?;
        // 彻底删除前必须无人引用：预期内的不适用，跳过并报告，不堵住其余技能
        if let Err(e) = unreferenced(&registry, key) {
            plan.data.skipped.push(Skipped::of(key, &e));
            continue;
        }
        plan.data.applied.push(key.clone());
        if let Some(dir) = occupancy.workdir() {
            push_face(&mut faces, Face::Store);
            plan.push(
                PlanStep::of(ActionKind::Remove, naming::store_subpath(key)),
                Act::Remove(dir.to_path_buf()),
            );
        }
        if let Some(dir) = occupancy.archive() {
            push_face(&mut faces, Face::Archive);
            plan.push(
                PlanStep::of(ActionKind::Remove, naming::archive_subpath(key)),
                Act::Remove(dir.to_path_buf()),
            );
        }
    }
    if !plan.data.applied.is_empty() {
        // 提交只有一步：一次删除一次提交，目标面在消息里一次点名全部删到的面
        let commit = batch_message(Verb::Remove, &faces, &plan.data.applied);
        plan.push(PlanStep::commit(commit), Act::None);
    }
    Ok(plan)
}

/// 目标面登记一次：同一条提交里每个面只点名一次
fn push_face(faces: &mut Vec<Face>, face: Face) {
    if !faces.contains(&face) {
        faces.push(face);
    }
}
