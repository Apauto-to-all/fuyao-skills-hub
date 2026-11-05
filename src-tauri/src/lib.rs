//! 桌面端装配：窗口、插件与桥命令挂载。业务逻辑全在 fsh-core，此处不含分支。

mod commands;
mod error;

use fsh_core::paths::HubPaths;
use tauri_plugin_window_state::StateFlags;

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
        // 窗口以 visible:false 建窗；插件恢复尺寸 / 位置 / 最大化后按 VISIBLE 标志 show，
        // 首次露脸即最终态，避免先按默认尺寸显示再跳变
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    StateFlags::SIZE
                        | StateFlags::POSITION
                        | StateFlags::MAXIMIZED
                        | StateFlags::VISIBLE,
                )
                .build(),
        )
        // 目录选择器：入库与使用表单的原生目录弹窗
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::capabilities,
            commands::init_hub,
            commands::list_skills,
            commands::show_skill,
            commands::diff_versions,
            commands::release_skill,
            commands::archive_skill,
            commands::restore_skill,
            commands::projects,
            commands::agents,
            commands::use_skill,
            commands::unuse_skill,
            commands::add_skill,
            commands::remove_skill,
            commands::doctor,
            commands::gc,
            commands::open_hub_dir,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
