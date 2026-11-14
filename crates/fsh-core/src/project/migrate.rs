//! 键迁移的项目面：登记簿旧键引用全部改写 + 链接级落点重链。
//! skill 侧只挪 store 目录；本模块算一次迁移计划（记录数、落点、动作清单），
//! 实跑按它落盘——先整份快照登记簿原文，再逐落点删旧建新。
//! 失败回滚由 skill 侧写骨架统一驱动：首个登记的撤销闭包把 store 目录挪回
//! 并重建已动过的落点，登记簿原文随后整文件还原。

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use super::ProjectError;
use super::dest::{
    Dest, DestGates, DestVerdict, check_destination, resolve_global, resolve_project,
};
use crate::hub;
use crate::link::{self, LinkKind};
use crate::lock::WriteToken;
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
        let resolved = match record.project_dir() {
            Some(dir) => resolve_project(&known, dir, &skill.agents, from)?,
            None => resolve_global(&known, &skill.agents, from)?,
        };
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

/// 落盘：按计划改写登记簿全部旧键引用，再逐落点删旧建新。
/// 首个登记的撤销闭包把 store 目录挪回并重建已动过的落点（此刻旧源已在场，
/// 重建必成）；登记簿原文快照随后整文件还原
pub(crate) fn migrate_registry_and_links(
    paths: &HubPaths,
    token: &WriteToken,
    ctx: &mut Ctx<'_>,
    plan: &MigratePlan,
    from: &str,
    to: &str,
) -> Result<Migrated, ProjectError> {
    let from_dir = paths.skill_workdir(from);
    let to_dir = paths.skill_workdir(to);

    // 复合撤销：先挪回 store 目录，再逐个把已动的落点删新链、重建旧链。
    // 已动清单在重链循环里实时补进（动手前就补，删了建不上的那处也在内）
    let touched: Rc<RefCell<Vec<(std::path::PathBuf, LinkKind)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let undo_touched = Rc::clone(&touched);
    let undo_from = from_dir.clone();
    let undo_to = to_dir.clone();
    ctx.on_rollback(move || {
        if let Err(e) = std::fs::rename(&undo_to, &undo_from) {
            tracing::warn!(path = %undo_to.display(), cause = %e, "回滚 store 目录挪动失败");
            return;
        }
        for (path, kind) in undo_touched.borrow().iter() {
            if let Err(e) = link::remove_link(path, *kind) {
                tracing::warn!(path = %path.display(), cause = %e, "回滚重链落点删除失败");
                continue;
            }
            if let Err(e) = link::create_dir_link(&undo_from, path) {
                tracing::warn!(path = %path.display(), cause = %e, "回滚重链落点重建失败");
            }
        }
    });

    // 登记簿改写：原文整份快照进回滚清单（在复合撤销之后登记，还原先于目录挪回执行）
    let original = std::fs::read_to_string(paths.projects_toml()).unwrap_or_default();
    rewrite_keys(paths, token, from, to)?;
    let restore_path = paths.projects_toml();
    let restore_text = original;
    ctx.on_rollback(move || {
        if let Err(e) = std::fs::write(&restore_path, &restore_text) {
            tracing::warn!(path = %restore_path.display(), cause = %e, "回滚登记簿还原失败");
        }
    });

    // 落点重链：逐个删旧建新。此刻旧源已挪走、新源在场，删旧链不碰内容、建新链直指新源
    let mut relinked = Vec::new();
    for site in &plan.links {
        link::remove_link(&site.path, site.kind)?;
        touched.borrow_mut().push((site.path.clone(), site.kind));
        link::create_dir_link(&to_dir, &site.path)?;
        relinked.push(site.path.display().to_string());
    }
    Ok(Migrated {
        registry_edits: plan.edits,
        relinked,
    })
}

/// 登记簿里全部旧键引用改写为新键：agents / link / last_used 原样保留
fn rewrite_keys(
    paths: &HubPaths,
    token: &WriteToken,
    from: &str,
    to: &str,
) -> Result<(), ProjectError> {
    let mut registry = hub::load_registry(paths)?;
    // 身份键先拷出来：登记键是从 registry 借来的，边走边改借不过检查
    let dirs: Vec<String> = registry
        .project_dirs()
        .map(|dir| dir.display().to_string())
        .collect();
    for dir in dirs {
        let Some(entry) = registry.project_mut(Path::new(&dir)) else {
            continue;
        };
        for skill in &mut entry.skills {
            if skill.key == from {
                skill.key = to.to_string();
            }
        }
    }
    for skill in &mut registry.global_mut().skills {
        if skill.key == from {
            skill.key = to.to_string();
        }
    }
    hub::save_registry(paths, &registry, token)?;
    Ok(())
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
