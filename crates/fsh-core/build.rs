//! 构建脚本只声明编译输入，不生成任何清单。
//! `include_dir` 在 stable 上不向 rustc 报告被内嵌的文件（track_path 只在
//! nightly feature 下生效），新增文件或技能目录不会进 dep-info，必须靠这里
//! 的 rerun-if-changed 触发重编译。

fn main() {
    // 路径相对包根：仓库 skills\ 整份是内嵌输入，内容增删都要重编译
    println!("cargo:rerun-if-changed=../../skills");
}
