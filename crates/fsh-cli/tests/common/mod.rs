//! 共享测试助手：起真实 fsh 子进程与统一信封解析，cli.rs 各类用例共用。
//! 放在 tests/common/ 下不会被 cargo 当作独立测试二进制编译。

use std::process::Command;

use serde_json::Value;

/// 起一次 fsh，返回 (stdout, 退出码)
pub fn fsh(args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_fsh"))
        .args(args)
        .output()
        .expect("起 fsh 子进程失败");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// 解析成功信封，断言 ok:true 并返回 data 节点
pub fn data_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(true), "信封: {v}");
    v["data"].clone()
}
