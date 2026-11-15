//! hub 装配：一条龙初始化（目录树 + .gitignore + hub.toml + git 仓库 + 首提交），
//! 收尾把仓库 `skills\` 的内嵌技能对齐进 `store\builtin\`。
//! 初始化幂等——仓库已存在时不重跑装配，缺件由修复分支补齐。
//! 装配与内置对齐排进同一份计划：一趟命令只取一次锁、只落一次盘，预演按这份
//! 计划列动作清单，实跑按同一份计划落盘，already 的判据两处同出一源。

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::materialize::{BuiltinPlan, builtin_plan, land_builtin};
use super::{HubConfig, HubError, gitignore_content, load_config};
use crate::bundled;
use crate::bundled::BuiltinSkill;
use crate::commit::{Face, Verb, message};
use crate::git;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
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
}

/// 装配计划里的业务数据：判据是仓库在不在——不在就首建，在就只补缺
enum AssemblyPlan {
    /// 首建：目录树、配置、仓库与首提交一次到位
    Fresh,
    /// 已建：只补缺件与缺目录
    Repair(RepairPlan),
}

impl AssemblyPlan {
    /// 目标已是装配完整的 hub：仓库在、且没有要补的缺件。
    /// 预演与实跑的 already 都取这一处判据
    fn already(&self) -> bool {
        match self {
            AssemblyPlan::Fresh => false,
            AssemblyPlan::Repair(repair) => repair.touched().is_empty(),
        }
    }
}

/// 排装配计划：仓库不在就是首建，后面几样都得从头写；在就只补缺
fn assembly_plan(paths: &HubPaths) -> Plan<AssemblyPlan> {
    if paths.root().join(".git").exists() {
        repair_plan(RepairPlan::of(paths))
    } else {
        fresh_plan()
    }
}

/// 首建计划：目录、配置、仓库各列各的，仓库建出后一次提交
fn fresh_plan() -> Plan<AssemblyPlan> {
    let mut plan = Plan::new(AssemblyPlan::Fresh);
    for face in crate::paths::FACES {
        plan.push(
            PlanStep::of(ActionKind::Create, format!("{face}/")),
            Act::None,
        );
    }
    plan.push(PlanStep::of(ActionKind::Write, ".gitignore"), Act::None);
    plan.push(PlanStep::of(ActionKind::Write, "hub.toml"), Act::None);
    plan.push(PlanStep::of(ActionKind::Init, "git"), Act::None);
    // 首建必然写下跟踪面，因此必然有一次提交
    plan.push(
        PlanStep::commit(message(Verb::Init, Face::Hub, "hub")),
        Act::None,
    );
    plan
}

/// 补缺计划：先补目录，再补缺件，有缺件才列机器提交那一步
fn repair_plan(repair: RepairPlan) -> Plan<AssemblyPlan> {
    let dirs: Vec<&'static str> = repair.dirs.iter().map(|(name, _)| *name).collect();
    let touched = repair.touched();
    let commit =
        (!touched.is_empty()).then(|| PlanStep::commit(message(Verb::Repair, Face::Hub, "hub")));
    let mut plan = Plan::new(AssemblyPlan::Repair(repair));
    for name in dirs {
        plan.push(
            PlanStep::of(ActionKind::Create, format!("{name}/")),
            Act::None,
        );
    }
    for name in &touched {
        plan.push(PlanStep::of(ActionKind::Repair, *name), Act::None);
    }
    if let Some(commit) = commit {
        plan.push(commit, Act::None);
    }
    plan
}

/// 一趟 init 的计划：装配与内置技能对齐各排各的步骤，业务数据留到落盘与里程碑
struct InitPlan<'a> {
    /// 装配部分
    assembly: AssemblyPlan,
    /// 内置技能对齐部分
    builtin: Vec<BuiltinPlan<'a>>,
}

/// 排 init 计划：装配步骤在前，内置对齐在后。同一趟落盘里第一条提交会带上
/// 全部改动，再列一条只会是空提交，故提交步只留一条
fn init_plan<'a>(
    paths: &HubPaths,
    skills: &'a [BuiltinSkill],
) -> Result<Plan<InitPlan<'a>>, HubError> {
    let assembly = assembly_plan(paths);
    let builtin = builtin_plan(paths, skills)?;
    let mut steps: Vec<PlanStep> = assembly.describe();
    steps.extend(builtin.describe());
    let mut plan = Plan::new(InitPlan {
        assembly: assembly.data,
        builtin: builtin.data,
    });
    let mut commit: Option<PlanStep> = None;
    for desc in steps {
        if desc.action == ActionKind::Commit {
            // 提交是收尾步骤：装配与内置对齐都落完才提交，计划里因此排在最后
            commit.get_or_insert(desc);
            continue;
        }
        plan.push(desc, Act::None);
    }
    if let Some(commit) = commit {
        plan.push(commit, Act::None);
    }
    Ok(plan)
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

/// init 命令的完整语义入口：幂等判定、预演动作清单与半截装配修复。
/// 预演与实跑各走一次装配骨架，计划同源；已是完整 hub 属幂等重跑，不算错误。
pub fn init(hub_root: &Path, dry_run: bool) -> Result<InitOutcome, HubError> {
    let paths = HubPaths::from_root(hub_root);
    let skills = bundled::skills();
    let result = write::run_assembly(
        hub_root,
        dry_run,
        |paths| init_plan(paths, &skills),
        dry_outcome,
        land_init,
        |plan, elapsed_ms| milestone(&plan, &paths, elapsed_ms),
    );
    // 初始化失败在 init 这个对外边界记一次，上层不重复记
    if let Err(e) = &result {
        tracing::error!(path = %paths.root().display(), cause = %e, "Fuyao Skills Hub 初始化失败");
    }
    result
}

/// 预演：零落盘，把计划翻成动作清单。already 与实跑同出一处——都看计划里的装配部分
fn dry_outcome(plan: Plan<InitPlan<'_>>, command: Vec<PlanStep>) -> InitOutcome {
    let mut actions = command;
    actions.extend(plan.describe());
    InitOutcome {
        dry_run: true,
        already: plan.data.assembly.already(),
        actions,
        ..InitOutcome::default()
    }
}

/// 按计划落盘：先装配，再把内置技能对齐进 store。
/// 首建在仓库建出后把句柄交回骨架，提交由骨架按计划里的提交步统一做；
/// 装配的每一步都幂等，盘面一律留在中途等重跑续完
fn land_init<'a>(
    ctx: &mut Ctx<'_>,
    plan: Plan<InitPlan<'a>>,
) -> Result<Applied<InitOutcome, InitPlan<'a>>, HubError> {
    let InitPlan { assembly, builtin } = plan.data;
    let paths = ctx.paths().clone();
    match &assembly {
        AssemblyPlan::Fresh => {
            for (_, dir) in assembly_dirs(&paths) {
                ctx.create_dir(&dir)?;
            }
            ctx.write_file(&paths.gitignore(), gitignore_content().as_bytes())?;
            // hub.toml 含用户身份：已存在就沿用，缺了才写默认值
            if load_config(&paths)?.is_none() {
                ctx.write_file(&paths.hub_toml(), HubConfig::v1().to_toml()?.as_bytes())?;
            }
            let repo = git::init(paths.root())?;
            ctx.attach_repo(repo);
        }
        AssemblyPlan::Repair(repair) => {
            for (_, dir) in &repair.dirs {
                ctx.create_dir(dir)?;
            }
            if repair.gitignore {
                ctx.write_file(&paths.gitignore(), gitignore_content().as_bytes())?;
            }
            if repair.hub_toml {
                ctx.write_file(&paths.hub_toml(), HubConfig::v1().to_toml()?.as_bytes())?;
            }
        }
    }
    land_builtin(ctx, &builtin)?;
    Ok(Applied {
        outcome: outcome_of(&assembly),
        milestones: InitPlan { assembly, builtin },
    })
}

/// 结果载荷：首建报 initialized，补了缺件报 initialized 与清单，只补目录报 already。
/// 预演与实跑共用这一处判据，already 因此只有一个来源
fn outcome_of(assembly: &AssemblyPlan) -> InitOutcome {
    match assembly {
        AssemblyPlan::Fresh => InitOutcome {
            initialized: true,
            ..InitOutcome::default()
        },
        // 补缺件报 initialized 与清单，只补目录报 already——判据与预演同一处
        AssemblyPlan::Repair(repair) if !assembly.already() => InitOutcome {
            initialized: true,
            repaired: repair
                .touched()
                .iter()
                .map(|part| (*part).to_string())
                .collect(),
            ..InitOutcome::default()
        },
        AssemblyPlan::Repair(_) => InitOutcome {
            already: true,
            ..InitOutcome::default()
        },
    }
}

/// 里程碑：装配结果与内置对齐各记一条，读 INFO 能重建这趟 init 做了什么
fn milestone(plan: &InitPlan<'_>, paths: &HubPaths, elapsed_ms: u64) {
    let path = paths.root();
    match &plan.assembly {
        // 初始化完成是状态迁移里程碑：读 INFO 即可重建 hub 何时建成、耗了多久
        AssemblyPlan::Fresh => tracing::info!(
            path = %path.display(),
            elapsed_ms = elapsed_ms,
            "Fuyao Skills Hub 初始化完成"
        ),
        AssemblyPlan::Repair(repair) if !repair.touched().is_empty() => tracing::info!(
            path = %path.display(),
            count = repair.touched().len(),
            elapsed_ms = elapsed_ms,
            "Fuyao Skills Hub 修复完成"
        ),
        // 幂等命中是决策点：重跑不装配属正常结果，DEBUG 留痕但不刷屏
        AssemblyPlan::Repair(_) => tracing::debug!(
            path = %path.display(),
            "Fuyao Skills Hub 已初始化，重复装配跳过"
        ),
    }
    let count = plan
        .builtin
        .iter()
        .filter(|skill| skill.described())
        .count();
    if count == 0 {
        // 内容一致是幂等命中：重跑 init 的正常结果，DEBUG 留痕不刷屏
        tracing::debug!(count = plan.builtin.len(), "内置技能内容一致，对齐跳过");
    } else {
        tracing::info!(
            path = %path.display(),
            count = count,
            elapsed_ms = elapsed_ms,
            "内置技能对齐完成"
        );
    }
}
