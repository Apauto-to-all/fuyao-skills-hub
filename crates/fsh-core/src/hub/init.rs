//! hub 装配：一条龙初始化（目录树 + .gitignore + hub.toml + git 仓库 + 首提交）。
//! 初始化幂等——仓库已存在时不重跑装配，缺件由修复分支补齐。

use std::fs;
use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use super::{GITIGNORE_CONTENT, HubConfig, HubError, load_config};
use crate::git;
use crate::paths::HubPaths;

/// 一条龙初始化 hub：目录树 → .gitignore → 默认 hub.toml → git 仓库 →
/// 以 hub.toml 身份首次提交。data\ 下已存在 .git 时拒绝且不触碰任何数据；
/// hub.toml 已存在则沿用其身份，不覆盖。
pub fn hub_init(install_dir: &Path) -> Result<HubPaths, HubError> {
    // 耗时起点：装配跨多次文件写入与 git 调用，完成时带 elapsed_ms 供诊断
    let started = Instant::now();
    let paths = HubPaths::new(install_dir);
    // 幂等闸门只认仓库：仓库在就绝不重跑装配（git::init 会拒绝重复初始化），
    // 缺件的补齐交给 repair，那里不覆盖任何用户数据
    if paths.root().join(".git").exists() {
        // 幂等命中是决策点：重跑不装配属正常结果，记 DEBUG 留痕但不刷屏
        tracing::debug!(path = %paths.root().display(), "Fuyao Skills Hub 已初始化，重复装配跳过");
        return Err(HubError::AlreadyHub(paths.root().display().to_string()));
    }
    for dir in [paths.store(), paths.export(), paths.agents()] {
        fs::create_dir_all(dir)?;
    }
    // 全局写锁：初始化也是写操作，与入库 / gc 互斥
    let _token = crate::lock::write_lock(&paths)?;
    // 锁内复查闸门：并发第二个 init 在此拿到 AlreadyHub（幂等），而不是 git 报错
    if paths.root().join(".git").exists() {
        // 锁内复查命中说明有并发初始化：与首道闸门分开记，便于定位竞争
        tracing::debug!(path = %paths.root().display(), "锁内复查命中已初始化，装配跳过");
        return Err(HubError::AlreadyHub(paths.root().display().to_string()));
    }
    fs::write(paths.gitignore(), GITIGNORE_CONTENT)?;
    // hub.toml 含用户身份：已存在就沿用，缺了才写默认值
    let cfg = match load_config(&paths)? {
        Some(cfg) => cfg,
        None => {
            let cfg = HubConfig::v1();
            fs::write(paths.hub_toml(), cfg.to_toml()?)?;
            cfg
        }
    };
    let repo = git::init(paths.root())?;
    git::commit_all(&repo, "init hub", &cfg.user.name, &cfg.user.email)?;
    // 初始化完成是状态迁移里程碑：读 INFO 即可重建 hub 何时建成、耗了多久
    tracing::info!(
        path = %paths.root().display(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Fuyao Skills Hub 初始化完成"
    );
    Ok(paths)
}

/// init 命令的结果载荷。五种形态经 serde 跳过空值后直出信封 data：
/// 首建 `{initialized}`、幂等重跑 `{already_initialized}`、
/// 补齐半截装配 `{initialized, repaired}`、
/// 未建的预演 `{dry_run, actions}`、已建的预演 `{dry_run, already_initialized}`
#[derive(Debug, Default, Serialize)]
pub struct InitOutcome {
    /// 预演报告标志（仅预演出现）
    #[serde(skip_serializing_if = "is_false")]
    pub dry_run: bool,
    /// 本次完成首次初始化或补齐了缺件（仅实跑出现）
    #[serde(skip_serializing_if = "is_false")]
    pub initialized: bool,
    /// 目标已是初始化过的 hub（实跑幂等重跑、已建的预演出现）
    #[serde(rename = "already_initialized", skip_serializing_if = "is_false")]
    pub already: bool,
    /// 本次补齐的缺件（仅修复出现）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repaired: Vec<String>,
    /// 预演动作清单（仅未建的预演出现）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// serde 跳过空值的判定（false 字段不入信封）
fn is_false(b: &bool) -> bool {
    !*b
}

/// init 命令的完整语义入口：幂等判定、预演动作清单与半截装配修复。
/// 实跑委托一条龙装配；已是完整 hub 属幂等重跑，不算错误。
pub fn init(install_dir: &Path, dry_run: bool) -> Result<InitOutcome, HubError> {
    let paths = HubPaths::new(install_dir);
    let result = run(&paths, install_dir, dry_run);
    // 初始化失败在 init 这个对外边界记一次，上层不重复记
    if let Err(e) = &result {
        tracing::error!(path = %paths.root().display(), cause = %e, "Fuyao Skills Hub 初始化失败");
    }
    result
}

/// 预演与实跑的分派：任一步失败都交给 init 的边界统一记日志
fn run(paths: &HubPaths, install_dir: &Path, dry_run: bool) -> Result<InitOutcome, HubError> {
    // 预演：零落盘，只按缺件列动作
    if dry_run {
        return Ok(plan(paths));
    }
    match hub_init(install_dir) {
        Ok(_) => Ok(InitOutcome {
            initialized: true,
            ..InitOutcome::default()
        }),
        // 仓库已存在：完整则幂等重跑，残缺则补齐缺件
        Err(HubError::AlreadyHub(_)) => {
            let repaired = repair(paths)?;
            if repaired.is_empty() {
                Ok(InitOutcome {
                    already: true,
                    ..InitOutcome::default()
                })
            } else {
                Ok(InitOutcome {
                    initialized: true,
                    repaired,
                    ..InitOutcome::default()
                })
            }
        }
        Err(e) => Err(e),
    }
}

/// 预演：零落盘，按缺件给出「会做什么」。
/// 全新装配与半截修复是两条不同的动作序列，判据同一个（缺件清单）
fn plan(paths: &HubPaths) -> InitOutcome {
    let missing = paths.missing_parts();
    if missing.is_empty() {
        return InitOutcome {
            dry_run: true,
            already: true,
            ..InitOutcome::default()
        };
    }
    // 缺 .git 就是全新装配：仓库不在，后面三样都得从头写
    let actions = if !paths.root().join(".git").exists() {
        vec![
            "create store/ export/ agents/".to_string(),
            "write .gitignore hub.toml".to_string(),
            "git init + first commit".to_string(),
        ]
    } else {
        vec![
            format!("repair hub: 补 {}", missing.join(" / ")),
            "commit repair".to_string(),
        ]
    };
    InitOutcome {
        dry_run: true,
        actions,
        ..InitOutcome::default()
    }
}

/// 补齐半截装配：只补缺、不覆盖用户数据（hub.toml 含提交身份）。
/// .gitignore 是派生物，内容与白名单不符即重写——自定的忽略规则应放 .git/info/exclude，
/// 那里不跟仓库走，也不会被这里覆盖
fn repair(paths: &HubPaths) -> Result<Vec<String>, HubError> {
    let started = Instant::now();
    let _token = crate::lock::write_lock(paths)?;
    // 目录缺失不影响可用性，但补回来是 init 该做的事；空目录不进提交，不产生提交
    for dir in [paths.store(), paths.export(), paths.agents()] {
        fs::create_dir_all(dir)?;
    }
    let mut touched: Vec<&'static str> = Vec::new();
    if !super::gitignore_is_canonical(paths) {
        fs::write(paths.gitignore(), GITIGNORE_CONTENT)?;
        touched.push(".gitignore");
    }
    if !paths.hub_toml().exists() {
        fs::write(paths.hub_toml(), HubConfig::v1().to_toml()?)?;
        touched.push("hub.toml");
    }
    if touched.is_empty() {
        return Ok(Vec::new());
    }
    let repo = git2::Repository::open(paths.root()).map_err(git::GitError::from)?;
    let user = load_config(paths)?.unwrap_or_default().user;
    // 只提交补回来的文件：工作区里其它未提交改动不属于这次修复
    let staged: Vec<&Path> = touched.iter().map(|p| Path::new(*p)).collect();
    git::commit_paths(&repo, &staged, "repair hub", &user.name, &user.email)?;
    tracing::info!(
        path = %paths.root().display(),
        count = touched.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Fuyao Skills Hub 修复完成"
    );
    Ok(touched.into_iter().map(str::to_string).collect())
}
