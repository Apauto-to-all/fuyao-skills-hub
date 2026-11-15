//! 检索范围限定：范围前缀解析。

use std::path::Path;

use super::SearchError;
use crate::paths::FACES;

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
    // 白名单只有 hub 布局里的 tracked 面：范围根必须是其中一面，或该面前缀内的路径
    let in_face = FACES
        .iter()
        .any(|face| p == *face || p.starts_with(&format!("{face}/")));
    if !in_face {
        return Err(invalid("在白名单可见范围之外"));
    }
    Ok(Some(p.to_string()))
}
