//! fsh-core：扶摇 skills hub 的纯逻辑库。
//! 业务逻辑 100% 位于本 crate：agent 只是数据，不含 UI。

pub mod agents;
pub(crate) mod bundled;
pub mod doctor;
pub mod error;
pub(crate) mod fingerprint;
pub mod git;
pub mod hub;
pub(crate) mod identity;
pub mod key;
pub mod link;
pub mod lock;
pub mod logging;
pub(crate) mod naming;
pub mod paths;
pub mod plan;
pub mod project;
pub mod search;
pub mod skill;
pub(crate) mod write;

pub use error::ErrorCode;

/// serde 跳过缺省值的判定：预演/实跑共用同一结果结构（见各操作模块的
/// Outcome），缺席字段不入信封，实跑与预演两种输出共用同一形状
pub(crate) fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
