//! 内容检索：对 hub 根做纯文本内容检索（`fsh search` 的 core 面）。
//! 遍历尊重 hub 白名单式 .gitignore——store、agents、hub.toml 可见，
//! export、logs、.lock 与根级散件天然不可见；忽略规则优先，glob 只能收窄。
//! 命中带键投影（store 外命中省略键）、行内字符偏移 spans（Unicode 标量值）
//! 与恒定排序（路径升序、行号升序）；limit 截断时 total 仍精确并标
//! truncated。上下文行参数只在本查询面存在，不改变命中结构与排序。
//! 归档技能无工作副本即遍历不到，不写排除逻辑。

use thiserror::Error;

use crate::ErrorCode;
use crate::git;

mod engine;
mod pattern;
mod scope;

pub use engine::{DEFAULT_LIMIT, SearchHit, SearchResult, search};
pub use pattern::SearchOptions;

/// 检索错误
#[derive(Debug, Error)]
pub enum SearchError {
    /// 文件系统操作失败（遍历与读文件）
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败（开仓校验，含 hub 未初始化）
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// 检索模式非法（正则或 glob 编译失败）
    #[error("检索模式非法: {0}")]
    PatternInvalid(String),
    /// 范围路径非法（含 `..`、绝对路径、反斜杠、盘符，或指向不存在的位置）
    #[error("检索范围路径非法: {0}")]
    PathInvalid(String),
}

impl ErrorCode for SearchError {
    fn error_code(&self) -> &'static str {
        match self {
            SearchError::Io(_) => "io",
            SearchError::Git(g) => g.error_code(),
            SearchError::PatternInvalid(_) => "search_pattern_invalid",
            SearchError::PathInvalid(_) => "search_path_invalid",
        }
    }
}

#[cfg(test)]
mod tests;
