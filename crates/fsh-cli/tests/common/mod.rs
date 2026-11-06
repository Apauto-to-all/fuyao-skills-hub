//! 共享测试助手：起真实 fsh 子进程、统一信封解析与临时环境搭建。
//! 放在 tests/common/ 下不会被 cargo 当作独立测试二进制编译。

// 每个测试二进制只用得上其中一部分助手，未用到的不该报警
#![allow(dead_code)]

use std::path::{Path, PathBuf};
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

/// 解析失败信封，断言 ok:false 并返回 error 节点
pub fn error_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(false), "信封: {v}");
    v["error"].clone()
}

/// 建测试环境：临时安装目录（hub 根 = <install>/data）+ 一个待入库 skill 源目录
/// （目录名必须等于 frontmatter name，故用 demo）
pub fn env_with_source() -> (tempfile::TempDir, PathBuf) {
    let install = tempfile::tempdir().unwrap();
    let src = install.path().join("demo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    (install, src)
}

/// hub 根路径（安装目录下 data\）
pub fn data_dir(install: &Path) -> String {
    install.join("data").display().to_string()
}

/// 同 env_with_source，但源目录 frontmatter 带版本号，并附带一个 references 文件
pub fn env_with_versioned_skill(version: &str) -> (tempfile::TempDir, PathBuf) {
    let (install, src) = env_with_source();
    std::fs::write(
        src.join("SKILL.md"),
        format!(
            "---
name: demo
version: \"{version}\"
description: 演示
---

正文
"
        ),
    )
    .unwrap();
    std::fs::create_dir_all(src.join("references")).unwrap();
    std::fs::write(src.join("references/a.txt"), "ref-a").unwrap();
    (install, src)
}
