//! GUI 的错误形状：序列化结果即前端 reject 值。

use fsh_core::ErrorCode;
use serde::Serialize;

/// 失败形状：`code` 是 core 的稳定 snake_case 码，`message` 是 core 的中文原话
#[derive(Debug, Clone, Serialize)]
pub struct GuiError {
    /// 稳定错误码，前端只按它分支（`lock_held` / `io` / ...）
    pub code: &'static str,
    /// 直接展示给用户的中文消息
    pub message: String,
}

/// 把 core 错误折成 GUI 形状：码透传、消息保原样，用户看到的与 CLI 一致
pub fn err(e: &(impl ErrorCode + std::fmt::Display)) -> GuiError {
    GuiError {
        code: e.error_code(),
        message: e.to_string(),
    }
}

/// 阻塞任务被取消（线程池 join 失败）时的统一错误
pub fn join_failed() -> GuiError {
    GuiError {
        code: "join_failed",
        message: "任务被取消".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试替身：只需要一个稳定码与一条中文消息
    struct FakeError;

    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "hub 正被其他进程写入（文件锁被占用）")
        }
    }

    impl ErrorCode for FakeError {
        fn error_code(&self) -> &'static str {
            "lock_held"
        }
    }

    #[test]
    fn err_passes_code_and_message_through() {
        let e = err(&FakeError);
        assert_eq!(e.code, "lock_held");
        assert_eq!(e.message, "hub 正被其他进程写入（文件锁被占用）");
    }

    #[test]
    fn join_failure_maps_to_stable_code() {
        let e = join_failed();
        assert_eq!(e.code, "join_failed");
        assert_eq!(e.message, "任务被取消");
    }
}
