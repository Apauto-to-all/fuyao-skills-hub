// 在 Windows 的 release 构建下隐藏附加控制台窗口，禁止删除!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    fsh_gui_lib::run()
}
