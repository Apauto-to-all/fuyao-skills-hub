//! 内容检索：对 hub 根做纯文本内容检索（`fsh search` 的 core 面）。
//! 遍历尊重 hub 白名单式 .gitignore——store、agents、hub.toml 可见，
//! logs、.lock 与根级散件天然不可见；忽略规则优先，glob 只能收窄。
//! 命中带键投影（store 外命中省略键）、行内字符偏移 spans（Unicode 标量值）
//! 与恒定排序（路径升序、行号升序）；limit 截断时 total 仍精确并标
//! truncated。上下文行展开见 context 模块：对命中集独立调用，
//! 不改变命中结构与排序。归档技能无工作副本即遍历不到，不写排除逻辑。

use thiserror::Error;

use crate::ErrorCode;
use crate::error::{CommonError, common_error_from};

mod context;
mod engine;
mod scope;

pub use context::{ContextLine, ContextRow, context_rows};
pub use engine::{DEFAULT_LIMIT, SearchHit, SearchResult, search};

/// 检索选项：检索面的全部开关
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// 相对路径限定范围（相对 hub 根，正斜杠分段）；None 即整库
    pub path: Option<String>,
    /// 强制忽略大小写；缺省 smart-case（模式全小写才忽略大小写）
    pub case_insensitive: bool,
    /// 模式按字面量解释（元字符不再当正则）
    pub literal: bool,
    /// 整词匹配（词边界锚定）
    pub whole_word: bool,
    /// glob 白名单（可重复）；只能在忽略规则筛过的可见集内收窄
    pub globs: Vec<String>,
    /// 命中上限（缺省 DEFAULT_LIMIT）；超出截断但 total 仍精确
    pub limit: Option<usize>,
}

/// 检索错误
#[derive(Debug, Error)]
pub enum SearchError {
    /// 底层失败（文件系统、开仓判定）
    #[error(transparent)]
    Common(#[from] CommonError),
    /// 检索模式非法（正则或 glob 编译失败）
    #[error("检索模式非法: {0}")]
    PatternInvalid(String),
    /// 范围路径非法（含 `..`、绝对路径、反斜杠、盘符，或指向不存在的位置）
    #[error("检索范围路径非法: {0}")]
    PathInvalid(String),
}

common_error_from!(SearchError: std::io::Error, crate::git::GitError);

impl ErrorCode for SearchError {
    fn error_code(&self) -> &'static str {
        match self {
            SearchError::Common(c) => c.error_code(),
            SearchError::PatternInvalid(_) => "search_pattern_invalid",
            SearchError::PathInvalid(_) => "search_path_invalid",
        }
    }
}

#[cfg(test)]
mod tests;
