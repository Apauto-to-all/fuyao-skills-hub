//! 桌面端装配：窗口、插件与桥命令挂载。业务逻辑全在 fsh-core，此处不含分支。

mod commands;
mod error;

use fsh_core::paths::HubPaths;

/// 托管状态：hub 路径族在启动时按可执行文件定位一次，命令直接克隆使用
pub struct Hub(pub HubPaths);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = HubPaths::discover(None);
    // 日志装一次即全局生效；guard 交给 tauri 状态管理，退出时刷盘
    let log_guard = fsh_core::logging::init_logging(&paths);

    tauri::Builder::default()
        .manage(log_guard)
        .manage(Hub(paths))
        .plugin(tauri_plugin_opener::init())
        // 窗口尺寸 / 位置 / 最大化状态跨重启记忆，由插件在退出与启动时读写
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            commands::capabilities,
            commands::init_hub,
            commands::open_hub_dir,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
