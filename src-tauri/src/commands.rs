//! 桥命令：只做参数搬运、线程池切换与错误折算，业务一律在 fsh-core。

use crate::Hub;
use crate::error::{GuiError, err, join_failed};
use fsh_core::agents::AgentInfo;
use fsh_core::doctor::DoctorReport;
use fsh_core::gc::GcReport;
use fsh_core::hub::InitOutcome;
use fsh_core::paths::HubPaths;
use fsh_core::project::{ProjectView, UnuseOutcome, UpgradeOutcome, UseOutcome};
use fsh_core::skill::{
    AddOutcome, ArchiveOutcome, DiffFile, ReleaseOutcome, ReleaseSource, RemoveOutcome,
    RestoreOutcome, SkillDetail, SkillSummary,
};
use serde::Serialize;
use std::path::PathBuf;
use tauri_plugin_opener::OpenerExt;

/// 环境快照：前端据此决定进引导页还是正常页
#[derive(Debug, Serialize)]
pub struct Capabilities {
    /// hub 根绝对路径（定位链 --hub > FSH_HUB > ~/.fuyao-skills-hub 解析出的根）
    pub hub_root: String,
    /// hub.toml 是否存在（初始化完成的判据）
    pub initialized: bool,
    /// 应用版本
    pub app_version: String,
    /// 运行平台（std::env::consts::OS）
    pub platform: String,
}

/// 环境识别：只查一次 hub.toml；文件系统读取一律不占命令线程
#[tauri::command(rename_all = "snake_case")]
pub async fn capabilities(hub: tauri::State<'_, Hub>) -> Result<Capabilities, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || snapshot(&paths))
        .await
        .map_err(|_| join_failed())
}

/// 初始化 hub：core 内已做幂等判定与写锁，这里只把它挪到阻塞线程池
#[tauri::command(rename_all = "snake_case")]
pub async fn init_hub(hub: tauri::State<'_, Hub>) -> Result<InitOutcome, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::hub::init(paths.root(), false))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 库清单：全部 skill 的薄行摘要（含归档态）
#[tauri::command(rename_all = "snake_case")]
pub async fn list_skills(hub: tauri::State<'_, Hub>) -> Result<Vec<SkillSummary>, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::skill::list(paths.root(), None))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 单个 skill 全量详情：frontmatter、版本时间线、引用与落点、SKILL.md 原文
#[tauri::command(rename_all = "snake_case")]
pub async fn show_skill(hub: tauri::State<'_, Hub>, key: String) -> Result<SkillDetail, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::skill::show(paths.root(), &key))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 两版本差异：core 已按文件聚合 unified patch，这里只搬运
#[tauri::command(rename_all = "snake_case")]
pub async fn diff_versions(
    hub: tauri::State<'_, Hub>,
    key: String,
    from: String,
    to: String,
) -> Result<Vec<DiffFile>, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::skill::diff(paths.root(), &key, &from, &to)
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 发版：把界面输入的号写回工作副本再打 tag；overwrite 只放行同号重发
#[tauri::command(rename_all = "snake_case")]
pub async fn release_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
    version: String,
    overwrite: bool,
) -> Result<ReleaseOutcome, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::skill::release(
            paths.root(),
            &key,
            ReleaseSource::Explicit(&version),
            overwrite,
            false,
        )
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 归档：删编辑面与导出目录，版本链保留，可恢复
#[tauri::command(rename_all = "snake_case")]
pub async fn archive_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
) -> Result<ArchiveOutcome, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::skill::archive(paths.root(), &key, false)
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 恢复：从最新 tag 重建工作副本，不产生新版本
#[tauri::command(rename_all = "snake_case")]
pub async fn restore_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
) -> Result<RestoreOutcome, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::skill::restore(paths.root(), &key, false)
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 项目清单：登记簿 + 每处落点的存在状态；解析失败的登记簿让整页报错
#[tauri::command(rename_all = "snake_case")]
pub async fn projects(hub: tauri::State<'_, Hub>) -> Result<Vec<ProjectView>, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::project::list(paths.root()))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// agent 清单：本机认得的 agent 及其配置
#[tauri::command(rename_all = "snake_case")]
pub async fn agents(hub: tauri::State<'_, Hub>) -> Result<Vec<AgentInfo>, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::agents::list(&paths))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 使用：把未归档 skill 按版本铺进目标集合的若干 agent；全局与项目目标可并存，
/// 两者都不给时 core 报 `no_target`；version 缺省表示跟随最新
#[tauri::command(rename_all = "snake_case")]
pub async fn use_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
    version: Option<String>,
    global: bool,
    project: Option<String>,
    agents: Vec<String>,
) -> Result<UseOutcome, GuiError> {
    let paths = hub.0.clone();
    let targets =
        fsh_core::project::Targets::new(global, project.map(PathBuf::from).into_iter().collect());
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::project::use_skill(
            paths.root(),
            &key,
            version.as_deref(),
            &agents,
            targets,
            false,
        )
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 版本切换：只改已有记录，把选中的目标切到这一版；全局与项目目标可并存，
/// version 缺省表示跟随最新
#[tauri::command(rename_all = "snake_case")]
pub async fn upgrade_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
    version: Option<String>,
    global: bool,
    projects: Vec<String>,
) -> Result<UpgradeOutcome, GuiError> {
    let paths = hub.0.clone();
    let targets =
        fsh_core::project::Targets::new(global, projects.into_iter().map(PathBuf::from).collect());
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::project::upgrade(
            paths.root(),
            &key,
            version.as_deref(),
            targets,
            false,
            false,
        )
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 解除：目标集合（可选全局 + 零到多个项目）一次撤完；agents 省缺即整体撤回，
/// 给了名单则只撤名单里的 agent，逐处检查落点后再删
#[tauri::command(rename_all = "snake_case")]
pub async fn unuse_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
    global: bool,
    projects: Vec<String>,
    agents: Option<Vec<String>>,
) -> Result<UnuseOutcome, GuiError> {
    let paths = hub.0.clone();
    let targets =
        fsh_core::project::Targets::new(global, projects.into_iter().map(PathBuf::from).collect());
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::project::unuse(paths.root(), &key, targets, agents.as_deref(), false, false)
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 入库：把源目录拷进 hub 并打首版 tag；prefix 覆盖默认键前缀
#[tauri::command(rename_all = "snake_case")]
pub async fn add_skill(
    hub: tauri::State<'_, Hub>,
    source: String,
    prefix: Option<String>,
) -> Result<AddOutcome, GuiError> {
    let paths = hub.0.clone();
    let source = PathBuf::from(source);
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::skill::add(paths.root(), &source, prefix.as_deref(), false)
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 移除：连版本 tag 一起删；被项目引用时 core 直接拒绝
#[tauri::command(rename_all = "snake_case")]
pub async fn remove_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
) -> Result<RemoveOutcome, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::skill::remove(paths.root(), &key, false))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 自检：七项检查只读，fail 计数由界面从 checks 里数
#[tauri::command(rename_all = "snake_case")]
pub async fn doctor(hub: tauri::State<'_, Hub>) -> Result<DoctorReport, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::doctor::doctor(paths.root()))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 回收：dry_run 只列清单；执行后返回实际释放字节
#[tauri::command(rename_all = "snake_case")]
pub async fn gc(hub: tauri::State<'_, Hub>, dry_run: bool) -> Result<GcReport, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::gc::gc(paths.root(), dry_run))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 在文件管理器里定位 hub 根；目录还不存在时先建出来，避免打开失败
#[tauri::command(rename_all = "snake_case")]
pub async fn open_hub_dir(
    app: tauri::AppHandle,
    hub: tauri::State<'_, Hub>,
) -> Result<(), GuiError> {
    let root = hub.0.root().to_path_buf();
    let dir = root.clone();
    tauri::async_runtime::spawn_blocking(move || std::fs::create_dir_all(&dir))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| GuiError {
            code: "io",
            message: e.to_string(),
        })?;
    app.opener()
        .reveal_item_in_dir(&root)
        .map_err(|e| GuiError {
            code: "opener",
            message: e.to_string(),
        })
}

/// 库页内容检索：查询词按字面量解释（键集合过滤语义），上下文行数供命中行展示。
/// 空词在桥上挡下（usage），其余错误由 core 折算
pub(crate) fn bridge_search(
    paths: &HubPaths,
    query: &str,
    context: Option<usize>,
) -> Result<fsh_core::search::SearchResult, GuiError> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err(GuiError {
            code: "usage",
            message: "检索词为空".to_string(),
        });
    }
    let opts = fsh_core::search::SearchOptions {
        literal: true,
        context,
        ..Default::default()
    };
    fsh_core::search::search(paths.root(), trimmed, &opts).map_err(|e| err(&e))
}

/// 内容检索桥命令：命中含键投影、上下文行与 spans
#[tauri::command(rename_all = "snake_case")]
pub async fn search_hub(
    hub: tauri::State<'_, Hub>,
    query: String,
    context: Option<usize>,
) -> Result<fsh_core::search::SearchResult, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || bridge_search(&paths, &query, context))
        .await
        .map_err(|_| join_failed())?
}

/// 环境快照的唯一构造处：命令与测试共用
fn snapshot(paths: &HubPaths) -> Capabilities {
    Capabilities {
        hub_root: paths.root().display().to_string(),
        initialized: paths.is_initialized(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
    }
}
#[cfg(test)]
#[path = "tests/capabilities.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/search.rs"]
mod search_tests;
