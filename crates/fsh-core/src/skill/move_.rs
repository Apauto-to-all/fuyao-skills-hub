//! 键迁移：store 内整目录改名 + 登记簿全部引用改写 + 链接级落点重链，一步完成。
//! 纯前缀重组——末段是身份，由 `--skill` 点名且原样保留，只换它前面的前缀；
//! 登记改写与落点重链住在 project 模块（那里才有落点解析），本模块只做三闸预检
//! 与 store 面挪动。

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SkillError;
use crate::commit::{Face, Verb, message};
use crate::key;
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::project::{self, Migrated};
use crate::select::{self, Selector};
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
    /// 新键（新前缀 + 原技能名）
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

/// 一次迁移的业务数据：两端键与项目面的迁移计划
struct MoveSet {
    /// 旧键（前缀 + 技能名）
    from: String,
    /// 新键（新前缀 + 同一个技能名）
    to: String,
    /// 项目面迁移计划：要改写的记录数与要重链的落点
    migrate: project::MigratePlan,
}

/// 键迁移：技能名即身份，只把它前面的前缀换成新前缀；先过三闸再动盘，
/// 中途失败按登记的逆序回滚。dry_run = true 时预演：只读不落盘、不取写锁。
pub fn move_skill(
    hub_root: &Path,
    prefix: Option<String>,
    name: &str,
    to: &str,
    dry_run: bool,
) -> Result<MoveOutcome, SkillError> {
    write::run(
        hub_root,
        dry_run,
        // 任何一步失败都按逆序撤销本次已动的盘面，不留半迁移状态
        OnFailure::Rollback,
        |paths| plan_move(paths, prefix.as_deref(), name, to),
        |plan, command| {
            let mut actions = command;
            actions.extend(plan.describe());
            MoveOutcome {
                dry_run: true,
                from: plan.data.from,
                to: plan.data.to,
                actions,
                ..MoveOutcome::default()
            }
        },
        |ctx, plan| {
            let MoveSet { from, to, migrate } = plan.data;
            // 登记簿改写排在重链之后：前面任何一步失败时它根本没被碰过
            let Migrated {
                registry_edits,
                relinked,
            } = project::rewrite_registry(ctx, &migrate, &from, &to)?;
            Ok(Applied {
                outcome: MoveOutcome {
                    from: from.clone(),
                    to: to.clone(),
                    registry_edits,
                    relinked,
                    ..MoveOutcome::default()
                },
                milestones: (from, to),
            })
        },
        |(from, to), elapsed_ms| {
            // 迁移跨挪目录、改登记与重链落点三段：一条 INFO 交代两端键与整段耗时
            tracing::info!(from = %from, to = %to, elapsed_ms = elapsed_ms, "skill 键迁移完成");
        },
    )
}

/// 两段拼成整键：前缀可省（=根）
fn key_of(prefix: Option<&str>, name: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}/{name}"),
        None => name.to_string(),
    }
}

/// 迁移计划：三闸全过才有计划；store 挪动、项目面重链步骤与提交步一次算清，
/// 实跑与预演共用这一份（落点三闸只在此判一次）
fn plan_move(
    paths: &HubPaths,
    prefix: Option<&str>,
    name: &str,
    to: &str,
) -> Result<Plan<MoveSet>, SkillError> {
    // 形状先判：前缀是多段键的形状，技能名恒为一段（小写字母数字连字符）
    let selector = Selector::named(prefix.map(str::to_string), vec![name.to_string()]);
    selector.check()?;
    // 新前缀只收前缀，末段仍由技能名给：空串与全空白按根处理
    let to_prefix = select::normalize(Some(to));
    if let Some(to_prefix) = to_prefix {
        key::validate(to_prefix)?;
    }
    let from = key_of(selector.prefix(), name);
    let to = key_of(to_prefix, name);
    // 闸一：旧键的工作副本必须在场（完全缺席报不存在，只有归档副本时
    // 收编成迁移侧文案，指路先 restore）
    let from_dir = match skill_occupancy(paths, &from).require_active() {
        Ok(dir) => dir,
        Err(crate::skill::ActiveError::Archived { key }) => {
            return Err(SkillError::ArchivedMove { key });
        }
        Err(crate::skill::ActiveError::SkillNotFound { key }) => {
            return Err(SkillError::SkillNotFound { key });
        }
    };
    // 闸二与闸三：新键的两个落点都必须空闲——store 落点被占报已存在，
    // 归档面躺着同名键报冲突（迁过去会让 restore 侧的两端判定打架）
    let to_occupancy = skill_occupancy(paths, &to);
    let to_dir = to_occupancy.require_free_store()?;
    to_occupancy.require_free_archive()?;
    let migrate = project::plan_migrate(paths, &from)?;
    // 重链步骤先算出来：步骤进计划，落点数据留在业务数据里
    let relinks = migrate.relink_steps(&from_dir, &to_dir);
    let register = (migrate.edits > 0)
        .then(|| PlanStep::of(ActionKind::Register, &from).landed_at(format!("{from} -> {to}")));
    let mut plan = Plan::new(MoveSet { from, to, migrate });
    // 挪 store 目录（新键的父层目录由写骨架的原语一并建出）
    plan.push(
        PlanStep::between(
            ActionKind::Move,
            naming::store_subpath(&plan.data.from),
            naming::store_subpath(&plan.data.to),
        )
        .targeting(&plan.data.from),
        Act::Move {
            from: from_dir,
            to: to_dir,
        },
    );
    for (desc, act) in relinks {
        plan.push(desc, act);
    }
    // 登记是收尾步骤：全部重链做完才改写登记簿，计划里因此排在重链之后
    if let Some(register) = register {
        plan.push(register, Act::None);
    }
    plan.push(
        PlanStep::commit(move_message(&plan.data.from, &plan.data.to)),
        Act::None,
    );
    Ok(plan)
}
