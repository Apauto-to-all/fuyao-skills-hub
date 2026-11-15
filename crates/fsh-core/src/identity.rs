//! 路径身份：同一目录的不同写法（斜杠方向、扩展前缀、大小写）归一成同一个身份。
//! 登记簿去重、落点查找与落点核验共用这一份判据。

use std::path::Path;

/// 路径身份：统一正斜杠、去掉 Windows 扩展前缀，Windows 下统一小写。
pub(crate) fn path_identity(path: &Path) -> String {
    // 反斜杠归一成单斜杠；Windows canonicalize 的 `\?\` 前缀去掉，
    // 使同一目录无论是否带扩展前缀都得到同一身份
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s
        .strip_prefix("//?/")
        .or_else(|| s.strip_prefix("//./"))
        .unwrap_or(&s);
    let s = s.strip_prefix('/').unwrap_or(s);
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_string()
    }
}

/// 登记簿表键：规范化绝对路径原样入键（不剥 Windows 扩展前缀——
/// 剥了前缀的超长路径做不了文件操作）。目录已不存在时按原写法取键：
/// 死记录也要能凭原写法读回来
pub(crate) fn project_key(project_dir: &Path) -> String {
    project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf())
        .display()
        .to_string()
}

/// 展示形态：只去掉 Windows 扩展前缀，斜杠方向与大小写保持原样。
/// 界面直接显示这个串，用户看到的应当是他自己认得的路径。
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    // UNC 的扩展形态是 `\\?\UNC\srv\share`，剥前缀后要还原成 `\\srv\share`
    if let Some(rest) = text
        .strip_prefix("\\\\?\\UNC\\")
        .or_else(|| text.strip_prefix("\\\\.\\UNC\\"))
    {
        return format!("\\\\{rest}");
    }
    text.strip_prefix("\\\\?\\")
        .or_else(|| text.strip_prefix("\\\\.\\"))
        .unwrap_or(&text)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{display_path, path_identity};
    use std::path::Path;

    /// 扩展前缀与斜杠方向都不改变身份
    #[test]
    fn identity_ignores_prefix_and_separators() {
        assert_eq!(
            path_identity(Path::new("\\\\?\\E:\\work\\alpha")),
            path_identity(Path::new("E:/work/alpha")),
            "带扩展前缀与正斜杠的写法应得到同一身份"
        );
    }

    /// 展示形态只剥前缀，其余原样保留
    #[test]
    fn display_strips_extended_prefix_only() {
        assert_eq!(
            display_path(Path::new("\\\\?\\E:\\work\\alpha")),
            "E:\\work\\alpha",
            "界面路径不该带扩展前缀"
        );
        assert_eq!(
            display_path(Path::new("E:/work/alpha")),
            "E:/work/alpha",
            "没有前缀时原样返回"
        );
    }
}
