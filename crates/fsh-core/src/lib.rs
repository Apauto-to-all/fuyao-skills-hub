//! fsh-core：扶摇 skills hub 的纯逻辑库。
//! 业务逻辑 100% 位于本 crate：不认识任何具体 Agent，也不含 UI。

pub mod backup;
pub mod deploy;
pub mod doctor;
pub mod gc;
pub mod git;
pub mod hub;
pub mod link;
pub mod lock;
pub mod paths;
pub mod skill;
