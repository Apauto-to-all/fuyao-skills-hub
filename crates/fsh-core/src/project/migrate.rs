//! 键迁移的项目面：链接级落点重链 + 登记簿旧键引用全部改写。
//! skill 侧先挪 store 目录；本模块算一次迁移计划（记录数、逐落点的重链步骤），
//! 重链步骤由写骨架按计划落盘，登记簿改写排在最后且只经写骨架的写入口改一次。
//! 每个落点的撤销由写骨架的重链原语就地登记（删新链、按旧源重建旧链）；
//! 登记簿排在最后，前面任何一步失败时它根本没被碰过。

use std::path::Path;

use super::ProjectError;
use super::anchor_of;
use super::dest::{Dest, DestGates, DestVerdict, check_destination};
use crate::hub;
use crate::link::LinkKind;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, PlanStep};
use crate::write::Ctx;

/// 一个落点的重链现场：路径、原链接方式与解析到它的 agent 名单
pub(crate) struct RelinkSite {
    /// 落点绝对路径
    pub(crate) path: std::path::PathBuf,
    /// 原链接方式
    kind: LinkKind,
    /// 解析到该落点的 agent 名单（动作清单带出）
    agents: Vec<String>,
}

/// 迁移计划：要改写的记录数与要重链的落点，预检一次算清
pub(crate) struct MigratePlan {
    /// 全部作用域里旧键记录的条数
    pub(crate) edits: usize,
    /// 链接级落点（copy 记录的落点不动、不入清单）
    links: Vec<RelinkSite>,
}

impl MigratePlan {
    /// 重链步骤：逐落点删旧建新，动作数据与描述同体。
    /// 两端源目录由 skill 侧给（挪 store 目录那一步在它那边）
    pub(crate) fn relink_steps(&self, from: &Path, to: &Path) -> Vec<(PlanStep, Act)> {
        self.links
            .iter()
            .map(|site| {
                (
                    PlanStep::of(ActionKind::Link, site.path.display().to_string())
                        .with_agents(site.agents.clone()),
                    Act::Relink {
                        path: site.path.clone(),
                        from: from.to_path_buf(),
                        to: to.to_path_buf(),
                        kind: site.kind,
                    },
                )
            })
            .collect()
    }

    /// 重链的落点清单：结果载荷用它
    pub(crate) fn relinked(&self) -> Vec<String> {
        self.links
            .iter()
            .map(|site| site.path.display().to_string())
            .collect()
    }
}

/// 迁移预检：读登记簿与 agent 清单，算出要改写的记录数与要重链的落点。
/// 链接级落点逐个过三闸（在场、形态与登记一致、靶子指旧 store 路径），
/// 任何一项不符即整体拒绝；copy 记录只数改写、不动落点
pub(crate) fn plan_migrate(paths: &HubPaths, from: &str) -> Result<MigratePlan, ProjectError> {
    let registry = hub::load_registry(paths)?;
    let known = crate::agents::list(paths)?;
    let old_source = paths.skill_workdir(from);

    let mut edits = 0;
    let mut links = Vec::new();
    for record in registry.records() {
        let skill = record.skill();
        if skill.key != from {
            continue;
        }
        edits += 1;
        if skill.link == LinkKind::Copy {
            continue;
        }
        let resolved = anchor_of(&record).resolve(&known, &skill.agents, from)?;
        for dest in resolved {
            ensure_live_link(&dest, skill.link, &old_source)?;
            links.push(RelinkSite {
                path: dest.path,
                kind: skill.link,
                agents: dest.agents,
            });
        }
    }

    Ok(MigratePlan { edits, links })
}

/// 落盘结论：改写的登记记录数与重链的落点清单
pub(crate) struct Migrated {
    /// 改写的登记记录数
    pub(crate) registry_edits: usize,
    /// 重链的落点清单
    pub(crate) relinked: Vec<String>,
}

/// 落盘：改写登记簿全部旧键引用，agents / link / last_used 原样保留。
/// 落在重链之后：骨架先按计划重链落点，重链失败时登记簿根本没被碰过
pub(crate) fn rewrite_registry(
    ctx: &mut Ctx<'_>,
    plan: &MigratePlan,
    from: &str,
    to: &str,
) -> Result<Migrated, ProjectError> {
    ctx.edit_registry(|registry| -> Result<(), ProjectError> {
        registry.rewrite_key(from, to);
        Ok(())
    })?;
    Ok(Migrated {
        registry_edits: plan.edits,
        relinked: plan.relinked(),
    })
}

/// 单落点三闸（形态、靶子）：落点不在场或与登记不符即整体拒绝——
/// 盲重链会销毁用户现场。copy 记录不走这里
fn ensure_live_link(dest: &Dest, kind: LinkKind, source: &Path) -> Result<(), ProjectError> {
    let gates = DestGates {
        kind,
        source,
        guard_copy: false,
        force: false,
    };
    match check_destination(dest, &gates)? {
        DestVerdict::Live => Ok(()),
        DestVerdict::Missing => Err(ProjectError::LinkKindMismatch {
            path: dest.path.display().to_string(),
            expected: kind,
            actual: "落点已不存在".to_string(),
        }),
    }
}
