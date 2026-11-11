//! 检索范围限定：范围前缀解析与 hub 装配判定。

use std::path::Path;

use super::SearchError;
use crate::git;

/// 范围解析：返回正斜杠相对前缀（None 即整库）。
/// 正斜杠分段是唯一写法；`..`、绝对路径、反斜杠、盘符、不存在的位置，
/// 以及 hub 根下被白名单 .gitignore 排除的子树（logs、.git 等）
/// 一律拒绝——遍历根恒为 hub 根，范围只做命中前缀过滤，永不开侧门
pub(super) fn scope_prefix(
    hub_root: &Path,
    path: Option<&str>,
) -> Result<Option<String>, SearchError> {
    let Some(p) = path else {
        return Ok(None);
    };
    let invalid = |reason: &str| SearchError::PathInvalid(format!("{p:?} {reason}"));
    if p.is_empty()
        || p.contains('\\')
        || p.contains("..")
        || Path::new(p).is_absolute()
        || p.contains(':')
    {
        return Err(invalid("不是相对 hub 根的正斜杠路径"));
    }
    let root = hub_root.join(p);
    if !root.is_dir() {
        return Err(invalid("不是 hub 内的目录"));
    }
    // 白名单只有 store、archive、agents 与 hub.toml 文件本身：范围根必须是
    // 其中之一的前缀内路径，否则在白名单外
    if !["store", "archive", "agents"].contains(&p)
        && !p.starts_with("store/")
        && !p.starts_with("archive/")
        && !p.starts_with("agents/")
    {
        return Err(invalid("在白名单可见范围之外"));
    }
    Ok(Some(p.to_string()))
}

/// 装配判定（复用 git::open_hub 的存在性判据）
pub(super) fn check_hub(hub_root: &Path) -> Result<(), SearchError> {
    git::open_hub(hub_root)?;
    Ok(())
}
