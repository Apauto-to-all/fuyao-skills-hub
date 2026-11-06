//! 输出塑形：命令名登记与成功 / 失败信封。--json 时 stdout 一行统一信封
//! `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 文本模式动作清单逐行、diff 逐文件输出 unified patch 原文（人读优先）、
//! 其余 pretty JSON。

use std::fmt::Display;

use fsh_core::ErrorCode;
use serde_json::{Value, json};

use crate::cli::Cmd;

/// 子命令的固定名字：日志里要稳定可查，不用 Debug（会带出参数值与类型括号）
pub(crate) fn cmd_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Init { .. } => "init",
        Cmd::Add { .. } => "add",
        Cmd::List { .. } => "list",
        Cmd::Show { .. } => "show",
        Cmd::Release { .. } => "release",
        Cmd::Archive { .. } => "archive",
        Cmd::Remove { .. } => "remove",
        Cmd::Restore { .. } => "restore",
        Cmd::Agents => "agents",
        Cmd::Projects => "projects",
        Cmd::Use { .. } => "use",
        Cmd::Unuse { .. } => "unuse",
        Cmd::Diff { .. } => "diff",
        Cmd::Gc { .. } => "gc",
        Cmd::Doctor => "doctor",
    }
}

/// 失败信封载荷：code 取 core 错误的稳定码（ErrorCode 契约，显式声明、
/// 包装变体透传底层真因），message 取 Display 文本
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

/// 任意 core 错误统一收敛为失败信封
pub(crate) fn fail(e: impl Display + ErrorCode) -> Failure {
    Failure {
        code: e.error_code(),
        message: e.to_string(),
    }
}

/// 成功输出：--json 一行信封；文本模式动作清单逐行、diff 逐文件输出
/// unified patch 原文（人读优先）、其余 pretty JSON
pub(crate) fn emit_ok(json: bool, data: &Value) {
    if json {
        println!("{}", json!({"ok": true, "data": data}));
        return;
    }
    if let Some(actions) = data.get("actions").and_then(Value::as_array) {
        for a in actions {
            println!("- {a}");
        }
        return;
    }
    // diff 的文件数组：patch 字段本身就是 unified diff 原文，直接打印
    if let Some(files) = data.get("files").and_then(Value::as_array) {
        for f in files {
            println!("diff {}", f["path"].as_str().unwrap_or(""));
            if let Some(p) = f["patch"].as_str() {
                print!("{p}");
            }
        }
        return;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".to_string())
    );
}

/// 失败输出：--json 一行信封；文本模式进 stderr（不污染 stdout 数据流）
pub(crate) fn emit_err(json: bool, f: &Failure) {
    if json {
        println!(
            "{}",
            json!({"ok": false, "error": {"code": f.code, "message": f.message}})
        );
        return;
    }
    eprintln!("错误 [{}]: {}", f.code, f.message);
}
