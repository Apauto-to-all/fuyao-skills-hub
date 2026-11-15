//! hub 装配：一条龙初始化（目录树 + .gitignore + hub.toml + git 仓库 + 首提交），
//! 收尾把仓库 `skills\` 的内嵌技能对齐进 `store\builtin\`。
//! 初始化幂等——仓库已存在时不重跑装配，缺件由修复分支补齐。
//! 预演与实跑出自同一份装配计划：预演把计划翻成动作清单，实跑按同一份计划落盘；
//! 两者共用写入口的共用骨架，锁序、封存手改与机器提交都只有一处实现。

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{HubConfig, HubError, gitignore_content, load_config, sync_builtin};
use crate::Actions;
use crate::bundled;
use crate::commit::{Face, Verb, message};
use crate::git;
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep, Steps};
use crate::write::{self, Applied, Ctx};

/// 装配要建出的三个目录：预演清单的写法与落盘路径。空目录不进提交，
/// 补它们不动仓库内容
fn assembly_dirs(paths: &HubPaths) -> Vec<(&'static str, PathBuf)> {
    crate::paths::FACES
        .iter()
        .map(|face| (*face, paths.root().join(face)))
        .collect()
}

/// 补缺计划：缺的目录、要重写的 .gitignore 与要写的 hub.toml
struct RepairPlan {
    /// 缺失的目录
    dirs: Vec<(&'static str, PathBuf)>,
    /// .gitignore 是否要重写：缺件，或内容与白名单不符。
    /// 它是派生物，内容与白名单不符即重写——自定的忽略规则应放 `.git/info/exclude`，
    /// 那里不跟仓库走，也不会被这里覆盖
    gitignore: bool,
    /// hub.toml 是否要写：缺失
    hub_toml: bool,
}

impl RepairPlan {
    /// 按盘面现状排计划：只列缺的那几样。hub.toml 含提交身份，只在缺失时写
    fn of(paths: &HubPaths) -> Self {
        Self {
            dirs: assembly_dirs(paths)
                .into_iter()
                .filter(|(_, dir)| !dir.is_dir())
                .collect(),
            gitignore: !super::gitignore_is_canonical(paths),
            hub_toml: !paths.hub_toml().exists(),
        }
    }

    /// 本次要补的缺件：结果载荷与里程碑日志都取这份清单
    fn touched(&self) -> Vec<&'static str> {
        [(".gitignore", self.gitignore), ("hub.toml", self.hub_toml)]
            .into_iter()
            .filter(|(_, fix)| *fix)
            .map(|(name, _)| name)
            .collect()
    }

    /// 补缺步骤：先补目录，再补缺件，有缺件才列机器提交那一步
    fn steps(&self) -> Vec<PlanStep> {
        let touched = self.touched();
        let mut steps: Vec<PlanStep> = self
            .dirs
            .iter()
            .map(|(name, _)| PlanStep::of(ActionKind::Create, format!("{name}/")))
            .collect();
        steps.extend(
            touched
                .iter()
                .map(|name| PlanStep::of(ActionKind::Repair, *name)),
        );
        if !touched.is_empty() {
            steps.push(PlanStep::commit(message(Verb::Repair, Face::Hub, "hub")));
        }
        steps
    }
}

/// 装配计划：判据是仓库在不在——不在就首建，在就只补缺。
/// 步骤随计划推出：预演直接列它，机器提交按其中的提交步落
enum AssemblyPlan {
    /// 首建：目录树、配置、仓库与首提交一次到位
    Fresh,
    /// 已建：只补缺件与缺目录
    Repair(RepairPlan),
}

impl Steps for AssemblyPlan {
    fn steps(&self) -> Vec<PlanStep> {
        match self {
            AssemblyPlan::Fresh => fresh_steps(),
            AssemblyPlan::Repair(repair) => repair.steps(),
        }
    }
}

/// 首建步骤：目录、配置、仓库各列各的，仓库建出后一次提交
fn fresh_steps() -> Vec<PlanStep> {
    let mut steps: Vec<PlanStep> = crate::paths::FACES
        .iter()
        .map(|face| PlanStep::of(ActionKind::Create, format!("{face}/")))
        .collect();
    steps.extend([
        PlanStep::of(ActionKind::Write, ".gitignore"),
        PlanStep::of(ActionKind::Write, "hub.toml"),
        PlanStep::of(ActionKind::Init, "git"),
        // 首建必然写下跟踪面，因此必然有一次提交
        PlanStep::commit(message(Verb::Init, Face::Hub, "hub")),
    ]);
    steps
}

/// 排装配计划：仓库不在就是首建，后面几样都得从头写
fn assembly_plan(paths: &HubPaths) -> AssemblyPlan {
    if paths.root().join(".git").exists() {
        AssemblyPlan::Repair(RepairPlan::of(paths))
    } else {
        AssemblyPlan::Fresh
    }
}

/// 装配落盘走的是哪条动作序列、补了哪几件
enum AssemblyLanded {
    /// 首建：仓库已建出并首次提交
    Fresh,
    /// 补缺：补上的缺件清单，只补目录时为空
    Repaired(Vec<&'static str>),
}

/// 按计划落盘，交回结果载荷与里程碑数据。首建在仓库建出后把句柄交回骨架，
/// 提交由骨架按计划里的提交步统一做；补缺只补缺件，只补目录时没有可提交的变更
fn land_assembly(
    ctx: &mut Ctx<'_>,
    plan: AssemblyPlan,
) -> Result<Applied<InitOutcome, AssemblyLanded>, HubError> {
    let paths = ctx.paths();
    let landed = match plan {
        AssemblyPlan::Fresh => {
            for (_, dir) in assembly_dirs(paths) {
                fs::create_dir_all(dir)?;
            }
            fs::write(paths.gitignore(), gitignore_content())?;
            // hub.toml 含用户身份：已存在就沿用，缺了才写默认值
            if load_config(paths)?.is_none() {
                fs::write(paths.hub_toml(), HubConfig::v1().to_toml()?)?;
            }
            let repo = git::init(paths.root())?;
            ctx.attach_repo(repo);
            AssemblyLanded::Fresh
        }
        AssemblyPlan::Repair(repair) => {
            for (_, dir) in &repair.dirs {
                fs::create_dir_all(dir)?;
            }
            if repair.gitignore {
                fs::write(paths.gitignore(), gitignore_content())?;
            }
            if repair.hub_toml {
                fs::write(paths.hub_toml(), HubConfig::v1().to_toml()?)?;
            }
            AssemblyLanded::Repaired(repair.touched())
        }
    };
    let outcome = outcome_of(&landed);
    Ok(Applied {
        outcome,
        milestones: landed,
    })
}

/// init 命令的结果载荷。五种形态经 serde 跳过空值后直出信封 data：
/// 首建 `{initialized}`、幂等重跑 `{already_initialized}`、
/// 补齐半截装配 `{initialized, repaired}`、
/// 未建的预演 `{dry_run, actions}`、
/// 已建的预演 `{dry_run, already_initialized}`（内置技能内容不一致时带 `actions`）
#[derive(Debug, Default, Serialize)]
pub struct InitOutcome {
    /// 预演报告标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 本次完成首次初始化或补齐了缺件（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub initialized: bool,
    /// 目标已是初始化过的 hub（实跑幂等重跑、已建的预演出现）
    #[serde(
        rename = "already_initialized",
        skip_serializing_if = "crate::is_default"
    )]
    pub already: bool,
    /// 本次补齐的缺件（仅修复出现）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repaired: Vec<String>,
    /// 预演动作清单（仅预演出现）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<PlanStep>,
}

impl Actions for InitOutcome {
    fn actions(&self) -> &[PlanStep] {
        &self.actions
    }
}

/// init 命令的完整语义入口：幂等判定、预演动作清单与半截装配修复。
/// 实跑按同一份装配计划落盘；已是完整 hub 属幂等重跑，不算错误。
pub fn init(hub_root: &Path, dry_run: bool) -> Result<InitOutcome, HubError> {
    let paths = HubPaths::from_root(hub_root);
    let result = if dry_run { plan(&paths) } else { land(&paths) };
    // 初始化失败在 init 这个对外边界记一次，上层不重复记
    if let Err(e) = &result {
        tracing::error!(path = %paths.root().display(), cause = %e, "Fuyao Skills Hub 初始化失败");
    }
    result
}

/// 预演：零落盘，把装配计划与内置技能对齐计划翻成动作清单。
/// 已建与否的判据是缺件清单，全新装配与半截修复是同一份计划的两条分支
fn plan(paths: &HubPaths) -> Result<InitOutcome, HubError> {
    let assembly = assembly_plan(paths);
    let mut actions: Vec<PlanStep> = assembly.steps();
    actions.extend(sync_builtin(paths, &bundled::skills(), true)?);
    Ok(InitOutcome {
        dry_run: true,
        already: paths.missing_parts().is_empty(),
        actions,
        ..InitOutcome::default()
    })
}

/// 实跑：装配按同一份计划落盘，收尾把内置技能对齐进 store。
/// 两步各自走共用骨架：装配在仓库尚未建出时开工，对齐在装配完成后开仓
fn land(paths: &HubPaths) -> Result<InitOutcome, HubError> {
    let outcome = write::run_assembly(
        paths.root(),
        |paths| Ok(assembly_plan(paths)),
        land_assembly,
        |landed, elapsed_ms| {
            let path = paths.root();
            match &landed {
                // 初始化完成是状态迁移里程碑：读 INFO 即可重建 hub 何时建成、耗了多久
                AssemblyLanded::Fresh => tracing::info!(
                    path = %path.display(),
                    elapsed_ms = elapsed_ms,
                    "Fuyao Skills Hub 初始化完成"
                ),
                AssemblyLanded::Repaired(parts) if !parts.is_empty() => tracing::info!(
                    path = %path.display(),
                    count = parts.len(),
                    elapsed_ms = elapsed_ms,
                    "Fuyao Skills Hub 修复完成"
                ),
                // 幂等命中是决策点：重跑不装配属正常结果，DEBUG 留痕但不刷屏
                AssemblyLanded::Repaired(_) => tracing::debug!(
                    path = %path.display(),
                    "Fuyao Skills Hub 已初始化，重复装配跳过"
                ),
            }
        },
    )?;
    // 内容一致时这一步零动作，已初始化 hub 的重跑仍只报告 already
    sync_builtin(paths, &bundled::skills(), false)?;
    Ok(outcome)
}

/// 结果载荷：首建报 initialized，补了缺件报 initialized 与清单，只补目录报 already
fn outcome_of(landed: &AssemblyLanded) -> InitOutcome {
    match landed {
        AssemblyLanded::Fresh => InitOutcome {
            initialized: true,
            ..InitOutcome::default()
        },
        AssemblyLanded::Repaired(parts) if !parts.is_empty() => InitOutcome {
            initialized: true,
            repaired: parts.iter().map(|part| (*part).to_string()).collect(),
            ..InitOutcome::default()
        },
        AssemblyLanded::Repaired(_) => InitOutcome {
            already: true,
            ..InitOutcome::default()
        },
    }
}
