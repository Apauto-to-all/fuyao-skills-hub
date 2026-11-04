#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志装一次即全局生效；guard 交给 tauri 状态管理，退出时刷盘
    let log_guard = fsh_core::logging::init_logging(&fsh_core::paths::HubPaths::discover(None));

    tauri::Builder::default()
        .manage(log_guard)
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
