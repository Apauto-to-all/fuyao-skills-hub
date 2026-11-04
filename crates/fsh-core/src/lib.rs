//! fsh-core：扶摇 skills hub 的纯逻辑库。
//! 业务逻辑 100% 位于本 crate：agent 只是数据，不含 UI。

pub mod agents;
pub mod doctor;
pub(crate) mod fingerprint;
pub mod gc;
pub mod git;
pub mod hub;
pub mod link;
pub mod lock;
pub mod logging;
pub(crate) mod naming;
pub mod paths;
pub mod project;
pub mod skill;

/// 错误稳定码契约：CLI / JSON 消费方靠 code 分支，禁止依赖自由文本。
/// 包装变体委托内层错误码，保证底层真因（如 tag_not_found）直达消费方
/// 而不被外层包装名（如 git）掩盖。
pub trait ErrorCode {
    /// snake_case 稳定码，对外契约的一部分，跨版本保持不变
    fn error_code(&self) -> &'static str;
}
