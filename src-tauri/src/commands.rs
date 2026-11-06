//! 桥命令：只做参数搬运、线程池切换与错误折算，业务一律在 fsh-core。

use crate::Hub;
use crate::error::{GuiError, err, join_failed};
use fsh_core::agents::AgentInfo;
use fsh_core::doctor::DoctorReport;
use fsh_core::gc::GcReport;
use fsh_core::hub::InitOutcome;
use fsh_core::paths::HubPaths;
use fsh_core::project::{ProjectView, UnuseReport, UseReport};
use fsh_core::skill::{AddReport, DiffFile, SkillDetail, SkillSummary};
use serde::Serialize;
use std::path::PathBuf;
use tauri_plugin_opener::OpenerExt;

/// 环境快照：前端据此决定进引导页还是正常页
#[derive(Debug, Serialize)]
pub struct Capabilities {
    /// hub 根绝对路径（可执行文件同级 data\）
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
    tauri::async_runtime::spawn_blocking(move || fsh_core::hub::init(paths.install(), false))
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
) -> Result<String, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::skill::release_version(paths.root(), &key, &version, overwrite)
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 归档：删编辑面与导出目录，版本链保留，可恢复
#[tauri::command(rename_all = "snake_case")]
pub async fn archive_skill(hub: tauri::State<'_, Hub>, key: String) -> Result<(), GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::skill::archive(paths.root(), &key))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 恢复：从最新 tag 重建工作副本，不产生新版本
#[tauri::command(rename_all = "snake_case")]
pub async fn restore_skill(hub: tauri::State<'_, Hub>, key: String) -> Result<(), GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::skill::restore(paths.root(), &key))
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

/// agent 清单：配置 + 从登记簿反查的使用记录
#[tauri::command(rename_all = "snake_case")]
pub async fn agents(hub: tauri::State<'_, Hub>) -> Result<Vec<AgentInfo>, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::agents::list(&paths))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 使用：把未归档 skill 按版本铺进项目的若干 agent；version 缺省表示跟随最新
#[tauri::command(rename_all = "snake_case")]
pub async fn use_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
    version: Option<String>,
    project: String,
    agents: Vec<String>,
) -> Result<UseReport, GuiError> {
    let paths = hub.0.clone();
    let project = PathBuf::from(project);
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::project::use_skill(paths.root(), &key, version.as_deref(), &agents, &project)
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 解除：整条技能记录撤回，逐处检查落点后再删
#[tauri::command(rename_all = "snake_case")]
pub async fn unuse_skill(
    hub: tauri::State<'_, Hub>,
    key: String,
    project: String,
) -> Result<UnuseReport, GuiError> {
    let paths = hub.0.clone();
    let project = PathBuf::from(project);
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::project::unuse(paths.root(), &key, &project, false)
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
) -> Result<AddReport, GuiError> {
    let paths = hub.0.clone();
    let source = PathBuf::from(source);
    tauri::async_runtime::spawn_blocking(move || {
        fsh_core::skill::add(paths.root(), &source, prefix.as_deref())
    })
    .await
    .map_err(|_| join_failed())?
    .map_err(|e| err(&e))
}

/// 移除：连版本 tag 一起删；被项目引用时 core 直接拒绝
#[tauri::command(rename_all = "snake_case")]
pub async fn remove_skill(hub: tauri::State<'_, Hub>, key: String) -> Result<(), GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::skill::remove(paths.root(), &key))
        .await
        .map_err(|_| join_failed())?
        .map_err(|e| err(&e))
}

/// 自检：七项检查只读，fail 计数由界面从 checks 里数
#[tauri::command(rename_all = "snake_case")]
pub async fn doctor(hub: tauri::State<'_, Hub>) -> Result<DoctorReport, GuiError> {
    let paths = hub.0.clone();
    tauri::async_runtime::spawn_blocking(move || fsh_core::doctor::doctor(paths.install()))
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
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn snapshot_reports_initialized_only_when_artifacts_exist() {
        let dir = std::env::temp_dir().join("fsh-gui-capabilities-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("建临时目录");
        let paths = HubPaths::from_root(&dir);

        let before = snapshot(&paths);
        assert!(!before.initialized);
        assert_eq!(before.hub_root, paths.root().display().to_string());

        // 只有 hub.toml 不算已初始化：半截装配必须暴露给引导页，否则用户
        // 看到的界面全是「已就绪」，点任何动作都回 hub_not_initialized
        fs::write(paths.hub_toml(), "").expect("写 hub.toml");
        assert!(!snapshot(&paths).initialized);

        fs::write(paths.gitignore(), "").expect("写 .gitignore");
        fs::create_dir_all(paths.root().join(".git")).expect("建 .git");
        assert!(snapshot(&paths).initialized);

        let _ = fs::remove_dir_all(&dir);
    }
}
