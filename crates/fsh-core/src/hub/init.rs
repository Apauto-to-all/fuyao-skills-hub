//! hub 装配：一条龙初始化（目录树 + .gitignore + hub.toml + git 仓库 + 首提交）。
//! 初始化幂等——对已初始化的 hub 拒绝重做且零破坏。

use std::fs;
use std::path::Path;

use serde::Serialize;

use super::{GITIGNORE_CONTENT, HubConfig, HubError};
use crate::git;
use crate::paths::HubPaths;

/// 一条龙初始化 hub：目录树 → .gitignore → 默认 hub.toml → git 仓库 →
/// 以 hub.toml 身份首次提交。data\ 下已存在 .git 时拒绝且不触碰任何数据。
pub fn hub_init(install_dir: &Path) -> Result<HubPaths, HubError> {
    let paths = HubPaths::new(install_dir);
    // 幂等闸门：已是 hub 则拒绝，且本函数在此之前不产生任何磁盘副作用
    if paths.root().join(".git").exists() {
        return Err(HubError::AlreadyHub(paths.root().display().to_string()));
    }
    for dir in [paths.store(), paths.export(), paths.agents()] {
        fs::create_dir_all(dir)?;
    }
    // 全局写锁：初始化也是写操作，与入库 / gc 互斥
    let _token = crate::lock::write_lock(&paths)?;
    // 锁内复查闸门：并发第二个 init 在此拿到 AlreadyHub（幂等），而不是 git 报错
    if paths.root().join(".git").exists() {
        return Err(HubError::AlreadyHub(paths.root().display().to_string()));
    }
    fs::write(paths.gitignore(), GITIGNORE_CONTENT)?;
    let cfg = HubConfig::v1();
    fs::write(paths.hub_toml(), cfg.to_toml()?)?;
    let repo = git::init(paths.root())?;
    git::commit_all(&repo, "init hub", &cfg.user.name, &cfg.user.email)?;
    Ok(paths)
}

/// init 命令的结果载荷。四种形态经 serde 跳过空值后直出信封 data：
/// 首建 `{initialized}`、幂等重跑 `{already_initialized}`、
/// 未建的预演 `{dry_run, actions}`、已建的预演 `{dry_run, already_initialized}`
#[derive(Debug, Default, Serialize)]
pub struct InitOutcome {
    /// 预演报告标志（仅预演出现）
    #[serde(skip_serializing_if = "is_false")]
    pub dry_run: bool,
    /// 本次完成首次初始化（仅实跑首建出现）
    #[serde(skip_serializing_if = "is_false")]
    pub initialized: bool,
    /// 目标已是初始化过的 hub（实跑幂等重跑、已建的预演出现）
    #[serde(rename = "already_initialized", skip_serializing_if = "is_false")]
    pub already: bool,
    /// 预演动作清单（仅未建的预演出现）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// serde 跳过空值的判定（false 字段不入信封）
fn is_false(b: &bool) -> bool {
    !*b
}

/// init 命令的完整语义入口：幂等判定与预演动作清单。
/// 实跑委托一条龙装配；已是 hub 属幂等重跑，不算错误。
pub fn init(install_dir: &Path, dry_run: bool) -> Result<InitOutcome, HubError> {
    let paths = HubPaths::new(install_dir);
    // 预演：零落盘，只判幂等与列动作
    if dry_run {
        if paths.root().join(".git").exists() {
            return Ok(InitOutcome {
                dry_run: true,
                already: true,
                ..InitOutcome::default()
            });
        }
        return Ok(InitOutcome {
            dry_run: true,
            actions: vec![
                "create store/ export/ agents/".to_string(),
                "write .gitignore hub.toml".to_string(),
                "git init + first commit".to_string(),
            ],
            ..InitOutcome::default()
        });
    }
    match hub_init(install_dir) {
        Ok(_) => Ok(InitOutcome {
            initialized: true,
            ..InitOutcome::default()
        }),
        // 幂等闸门命中：原样报告，不破坏任何数据
        Err(HubError::AlreadyHub(_)) => Ok(InitOutcome {
            already: true,
            ..InitOutcome::default()
        }),
        Err(e) => Err(e),
    }
}
