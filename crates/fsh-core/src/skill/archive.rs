//! 归档：把技能的工作副本整目录移入 hub 平级的归档目录（archive\<键>\，
//! 与 store 同构、tracked）；技能仍被引用时拒绝。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{SkillError, ensure_unreferenced, skill_status};
use crate::commit::{Face, Verb, message};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep};
use crate::write::{self, Applied, OnFailure};

/// 预演载荷：预演标志置真，动作清单即计划里的步骤
impl From<Vec<PlanStep>> for ArchiveOutcome {
    fn from(actions: Vec<PlanStep>) -> Self {
        Self {
            dry_run: true,
            actions,
            ..Self::default()
        }
    }
}

/// archive 的结果载荷：实跑直出 `{key, archived}`，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，实跑与预演两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 归档的 skill 唯一键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 归档完成标志（仅实跑出现，恒为 true）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub archived: bool,
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}

/// 归档：先查登记簿，被项目引用即拒绝；通过后把工作副本整目录移入
/// 归档目录（目标键已占用报 conflict），并自动提交。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn archive(hub_root: &Path, key: &str, dry_run: bool) -> Result<ArchiveOutcome, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    write::run(
        hub_root,
        dry_run,
        // 目录移动失败时工作副本还在原处：盘上不留移了一半的归档面
        OnFailure::Rollback,
        |paths| prepare_archive(paths, key),
        |plan| ArchiveOutcome::from(plan.actions),
        |_ctx, plan| {
            let ArchivePlan { from, to, .. } = plan;
            // 嵌套键的父层目录一并建出（archive/builtin/... 这类中间层）
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&from, &to)?;
            let commits = vec![message(Verb::Archive, Face::Store, key)];
            Ok(Applied {
                outcome: ArchiveOutcome {
                    key: key.into(),
                    archived: true,
                    ..ArchiveOutcome::default()
                },
                commits,
                milestones: (),
            })
        },
        |(), elapsed_ms| {
            // 归档跨移动目录与机器提交两段外部操作：一条 INFO 交代键与整段耗时
            tracing::info!(key = %key, elapsed_ms = elapsed_ms, "skill 归档完成");
        },
    )
}

/// 归档预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct ArchivePlan {
    /// 工作副本目录：移动的来源
    from: PathBuf,
    /// 归档目录：移动的落点
    to: PathBuf,
    /// 将执行的步骤清单
    actions: Vec<PlanStep>,
}

/// 归档预检：确认目标活跃、无人引用、归档侧无同名键；动作清单在这里一次成形
fn prepare_archive(paths: &HubPaths, key: &str) -> Result<ArchivePlan, SkillError> {
    let status = skill_status(paths, key);
    status.require_active()?;
    // 归档前必须无人引用：项目或全局任一段在用就拒绝，不给强制开关
    ensure_unreferenced(paths, key)?;
    // 归档侧同名键已占用：统一 conflict，不做静默改名
    if paths.skill_archived(key).exists() {
        return Err(SkillError::Conflict { key: key.into() });
    }
    let actions = vec![
        PlanStep::between(
            ActionKind::Move,
            crate::naming::store_subpath(key),
            crate::naming::archive_subpath(key),
        )
        .targeting(key),
        PlanStep::commit(message(Verb::Archive, Face::Store, key)),
    ];
    Ok(ArchivePlan {
        from: paths.skill_workdir(key),
        to: paths.skill_archived(key),
        actions,
    })
}
