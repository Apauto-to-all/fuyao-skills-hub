// 把 assets/icon.ico 编进 exe 的 Windows 资源段，供资源管理器与任务栏读取。
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=../../assets/icon.rc");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    embed_resource::compile("../../assets/icon.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("编译 Windows 资源段失败：需要 Windows SDK 提供的 rc.exe");
}
