//! 键迁移：store 内整目录改名 + 登记簿全部引用改写 + 链接级落点重链，一步完成。
//! 纯前缀重组——末段是身份，末段不等直接拒绝；登记改写与落点重链住在
//! project 模块（那里才有落点解析），本模块只做四闸预检与 store 面挪动。

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SkillError;
use crate::commit::{Face, Verb, message};
use crate::key;
use crate::naming;
use crate::plan::{ActionKind, PlanStep, Steps};
use crate::project::{self, Migrated};
use crate::skill::state::skill_occupancy;
use crate::write::{self, Applied, OnFailure};

/// move 的结果载荷：预演直出动作清单，实跑直出两端键、改写数与重链落点；
/// 缺席字段经 serde 跳过，两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 旧键（恒出现：预演也要知道自己从哪搬）
    pub from: String,
    /// 新键（仅实跑出现；预演里它就是调用方给的第二个参数）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub to: String,
    /// 改写的登记记录数（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub registry_edits: usize,
    /// 重链的落点清单（仅实跑出现；copy 落点不重链、不入清单）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub relinked: Vec<String>,
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}

/// 迁移提交消息：对象只含两端相对键，不含任何路径
pub(super) fn move_message(from: &str, to: &str) -> String {
    message(Verb::Move, Face::Store, &format!("{from} -> {to}"))
}

/// 键迁移：先过四闸再动盘，中途失败按登记的逆序回滚。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn move_skill(
    hub_root: &Path,
    from: &str,
    to: &str,
    dry_run: bool,
) -> Result<MoveOutcome, SkillError> {
    // 两键形状都先判：写错的键当场点名，不落到「技能不存在」
    key::validate(from)?;
    key::validate(to)?;
    write::run(
        hub_root,
        dry_run,
        // 任何一步失败都按逆序撤销本次已动的盘面，不留半迁移状态
        OnFailure::Rollback,
        |paths| plan_move(paths, from, to),
        |dry| MoveOutcome {
            dry_run: true,
            from: from.into(),
            actions: dry.all_steps(),
            ..MoveOutcome::default()
        },
        |ctx, plan| {
            // 挪 store 目录（新键的父层目录一并建出），撤销就地登记
            ctx.move_dir(&plan.from_dir, &plan.to_dir)?;
            // 落点重链 + 登记簿改写：各自的撤销在动盘那一刻登记
            let Migrated {
                registry_edits,
                relinked,
            } = project::migrate_registry_and_links(ctx, &plan.migrate, from, to)?;

            Ok(Applied {
                outcome: MoveOutcome {
                    from: from.into(),
                    to: to.into(),
                    registry_edits,
                    relinked,
                    ..MoveOutcome::default()
                },
                milestones: (from.to_string(), to.to_string()),
            })
        },
        |(from, to), elapsed_ms| {
            // 迁移跨挪目录、改登记与重链落点三段：一条 INFO 交代两端键与整段耗时
            tracing::info!(from = %from, to = %to, elapsed_ms = elapsed_ms, "skill 键迁移完成");
        },
    )
}

/// 迁移计划：四闸全过才有计划；store 挪动两端与项目面子计划一次算清，
/// 实跑与预演共用这一份（落点三闸只在此判一次）
struct MovePlan {
    /// 旧键的 store 工作副本目录：移动的来源
    from_dir: std::path::PathBuf,
    /// 新键的 store 目录：移动的落点
    to_dir: std::path::PathBuf,
    /// 项目面子计划：登记改写与落点重链
    migrate: project::MigratePlan,
    /// 将执行的步骤清单
    actions: Vec<PlanStep>,
}

impl Steps for MovePlan {
    fn steps(&self) -> Vec<PlanStep> {
        self.actions.clone()
    }
}

/// 迁移四闸（一次性，任何一项不过即报错且磁盘零改动）：
/// 末段相等（身份不变）、旧键在 store 在场、新键 store 路径空闲、键格式合法。
/// 键格式在入口先判；这里依次判末段、来源、落点
fn plan_move(paths: &crate::paths::HubPaths, from: &str, to: &str) -> Result<MovePlan, SkillError> {
    // 闸一：末段即身份，move 只重组前缀
    if key::last_segment(from) != key::last_segment(to) {
        return Err(SkillError::IdentityImmutable {
            from: from.into(),
            to: to.into(),
        });
    }
    // 闸二：旧键的工作副本必须在场（完全缺席报不存在，只有归档副本时
    // 收编成迁移侧文案，指路先 restore）
    let from_dir = match skill_occupancy(paths, from).require_active() {
        Ok(dir) => dir,
        Err(crate::skill::ActiveError::Archived { key }) => {
            return Err(SkillError::ArchivedMove { key });
        }
        Err(crate::skill::ActiveError::SkillNotFound { key }) => {
            return Err(SkillError::SkillNotFound { key });
        }
    };
    // 闸三与闸四：新键的两个落点都必须空闲——store 落点被占报已存在，
    // 归档面躺着同名键报冲突（迁过去会让 restore 侧的两端判定打架）
    let to_occupancy = skill_occupancy(paths, to);
    let to_dir = to_occupancy.require_free_store()?;
    to_occupancy.require_free_archive()?;
    // 动作清单：挪目录 →（计划里的改登记与重链步骤原序跟随）→ 提交
    let migrate = project::plan_migrate(paths, from, to)?;
    let mut actions = Vec::new();
    actions.push(
        PlanStep::between(
            ActionKind::Move,
            naming::store_subpath(from),
            naming::store_subpath(to),
        )
        .targeting(from),
    );
    actions.extend(migrate.actions().iter().cloned());
    actions.push(PlanStep::commit(move_message(from, to)));
    Ok(MovePlan {
        from_dir,
        to_dir,
        migrate,
        actions,
    })
}
