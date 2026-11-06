//! 导出目录树遍历：按「键前缀层 / 版本段」结构走查 export\\，产出回收候选。

use std::fs;
use std::path::Path;

/// 递归收集导出候选：目录名是版本段（三段数字）即 (键前缀, 版本, 路径)
/// 候选，否则视为键前缀层继续下钻；版本目录内容是导出副本、不再下钻
pub(super) fn collect_version_dirs(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, String, std::path::PathBuf)>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if crate::version::Version::parse(&name).is_some() {
            out.push((prefix.to_string(), name, p));
        } else {
            collect_version_dirs(&p, &rel, out);
        }
    }
}
