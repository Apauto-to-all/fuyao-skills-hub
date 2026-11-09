//! 检索查询面：选项结构。

/// 检索选项（CLI 与 GUI 共用；上下文行参数只在本面，不经 CLI 暴露）
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
    /// 命中行上下文行数（供 GUI 展示；不改变命中结构与排序）
    pub context: Option<usize>,
}
