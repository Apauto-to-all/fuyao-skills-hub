//! 彻底删除：两侧副本（store 工作副本与 archive 归档副本）哪侧在场删哪侧，
//! 被项目引用即拒绝，无强制开关。

use std::path::Path;

use super::outcome::{SkillOutcome, dry};
use super::{SkillError, ensure_unreferenced, skill_occupancy};
use crate::commit::{Face, Verb, message_faces};
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::write::{self, Applied, OnFailure};

/// 彻底删除：先查登记簿（被引用即拒绝），再删在场的副本——两侧都在就两侧一起删；
/// 历史 commit 留在审计日志里。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn remove(hub_root: &Path, key: &str, dry_run: bool) -> Result<SkillOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    write::run(
        hub_root,
        dry_run,
        // 删除不回滚：删到一半失败时已删的部分保持删掉，重跑即可续完
        OnFailure::Resume,
        |paths| prepare_remove(paths, key),
        dry,
        |_ctx, _plan| {
            // 一次删除只产生一条提交：两处删除在盘上已一并完成，提交步因此
            // 在计划里只写一条，目标面一次点名全部删到的面
            Ok(Applied {
                outcome: SkillOutcome {
                    key: key.into(),
                    removed: true,
                    ..SkillOutcome::default()
                },
                milestones: (),
            })
        },
        |(), elapsed_ms| {
            // 移除不可逆：键与整段耗时一并落 INFO，事后可核对删了什么
            tracing::info!(key = %key, elapsed_ms = elapsed_ms, "skill 移除完成");
        },
    )
}

/// remove 预检：确认至少一侧副本在场、无人引用；计划在这里一次成形
fn prepare_remove(paths: &HubPaths, key: &str) -> Result<Plan<()>, SkillError> {
    let occupancy = skill_occupancy(paths, key);
    // 至少一侧在场：两侧皆无报 skill_not_found
    occupancy.require_removable()?;
    // 彻底删除前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    let mut faces = Vec::new();
    let mut plan = Plan::new(());
    if let Some(dir) = occupancy.workdir() {
        faces.push(Face::Store);
        plan.push(
            PlanStep::of(ActionKind::Remove, naming::store_subpath(key)),
            Act::Remove(dir.to_path_buf()),
        );
    }
    if let Some(dir) = occupancy.archive() {
        faces.push(Face::Archive);
        plan.push(
            PlanStep::of(ActionKind::Remove, naming::archive_subpath(key)),
            Act::Remove(dir.to_path_buf()),
        );
    }
    // 提交只有一步：一次删除一次提交，目标面在消息里一次点名全部删到的面
    plan.push(
        PlanStep::commit(message_faces(Verb::Remove, &faces, key)),
        Act::None,
    );
    Ok(plan)
}
