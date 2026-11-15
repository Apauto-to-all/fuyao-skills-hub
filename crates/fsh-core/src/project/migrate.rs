//! 键迁移的项目面：链接级落点重链 + 登记簿旧键引用全部改写。
//! skill 侧先挪 store 目录；本模块算一次迁移计划（记录数、落点、动作清单），
//! 实跑按它落盘——逐个落点删旧建新，最后改写登记簿。
//! 每个落点在改盘的那一刻登记自己的撤销（删新链、按旧源重建旧链）；
//! 登记簿排在最后且只经写骨架的写入口改一次，失败时按登记的逆序回滚。

use std::path::Path;

use super::ProjectError;
use super::anchor_of;
use super::dest::{Dest, DestGates, DestVerdict, check_destination};
use crate::hub;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep};
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

/// 迁移计划：要改写的记录数、要重链的落点与动作清单，预检一次算清
pub(crate) struct MigratePlan {
    /// 全部作用域里旧键记录的条数
    pub(crate) edits: usize,
    /// 链接级落点（copy 记录的落点不动、不入清单）
    links: Vec<RelinkSite>,
    /// 动作清单（不含挪目录与提交，那两步由 skill 侧补）
    actions: Vec<PlanStep>,
}

impl MigratePlan {
    /// 预演动作清单
    pub(crate) fn actions(&self) -> &[PlanStep] {
        &self.actions
    }
}

/// 迁移预检：读登记簿与 agent 清单，算出要改写的记录数、要重链的落点与动作清单。
/// 链接级落点逐个过三闸（在场、形态与登记一致、靶子指旧 store 路径），
/// 任何一项不符即整体拒绝；copy 记录只数改写、不动落点
pub(crate) fn plan_migrate(
    paths: &HubPaths,
    from: &str,
    to: &str,
) -> Result<MigratePlan, ProjectError> {
    let registry = hub::load_registry(paths)?;
    let known = crate::agents::list(paths)?;
    let old_source = paths.skill_workdir(from);

    let mut edits = 0;
    let mut links = Vec::new();
    let mut actions = Vec::new();
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

    if edits > 0 {
        actions.push(PlanStep::of(ActionKind::Register, from).landed_at(format!("{from} -> {to}")));
    }
    for site in &links {
        actions.push(
            PlanStep::of(ActionKind::Link, site.path.display().to_string())
                .with_agents(site.agents.clone()),
        );
    }
    Ok(MigratePlan {
        edits,
        links,
        actions,
    })
}

/// 落盘结论：改写的登记记录数与重链的落点清单
pub(crate) struct Migrated {
    /// 改写的登记记录数
    pub(crate) registry_edits: usize,
    /// 重链的落点清单
    pub(crate) relinked: Vec<String>,
}

/// 落盘：逐落点删旧建新，最后改写登记簿全部旧键引用。
/// 每个落点动手之前先登记自己的撤销（删新链、按旧源重建旧链）；目录挪回登记
/// 得更早，逆序执行时排在最后，重建出的旧链随之落回在场的旧源。
/// 登记簿排在最后，前面任何一步失败时它根本没被碰过
pub(crate) fn migrate_registry_and_links(
    ctx: &mut Ctx<'_>,
    plan: &MigratePlan,
    from: &str,
    to: &str,
) -> Result<Migrated, ProjectError> {
    let from_dir = ctx.paths().skill_workdir(from);
    let to_dir = ctx.paths().skill_workdir(to);

    // 落点重链：逐个删旧建新。此刻旧源已挪走、新源在场，删旧链不碰内容、建新链直指新源
    let mut relinked = Vec::new();
    for site in &plan.links {
        link::remove_link(&site.path, site.kind)?;
        let path = site.path.clone();
        let kind = site.kind;
        let old_source = from_dir.clone();
        ctx.on_rollback(move || {
            if let Err(e) = link::remove_link(&path, kind) {
                tracing::warn!(path = %path.display(), cause = %e, "回滚重链落点删除失败");
                return;
            }
            if let Err(e) = link::create_dir_link(&old_source, &path) {
                tracing::warn!(path = %path.display(), cause = %e, "回滚重链落点重建失败");
            }
        });
        link::create_dir_link(&to_dir, &site.path)?;
        relinked.push(site.path.display().to_string());
    }

    // 登记簿改写：全部旧键引用改指新键，agents / link / last_used 原样保留
    ctx.edit_registry(|registry| -> Result<(), ProjectError> {
        registry.rewrite_key(from, to);
        Ok(())
    })?;
    Ok(Migrated {
        registry_edits: plan.edits,
        relinked,
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
