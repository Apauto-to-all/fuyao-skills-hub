//! 内置技能表：编译期把仓库 `skills\` 整份嵌进二进制，发布产物仍只有一个 exe。
//! 内置技能的标识就是 `skills\` 下一级目录名；本模块只读，不落任何外带资源。

use std::path::Path;

use include_dir::{Dir, include_dir};

/// 仓库 `skills\` 目录的编译期内嵌：路径写错在编译期即失败，
/// 目录里任意文件与子目录都随二进制发布
static SKILLS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../skills");

/// 一个内置技能：目录名与整份文件
pub(crate) struct BuiltinSkill {
    /// 内置技能名 = `skills\` 下的目录名 = 键末段
    pub(crate) name: String,
    /// 技能目录内的全部文件
    pub(crate) files: Vec<BuiltinFile>,
}

/// 内置技能里的一个文件
pub(crate) struct BuiltinFile {
    /// 相对技能根的路径，`/` 分隔
    pub(crate) path: String,
    /// 文件字节原样（文本与二进制都不转码）
    pub(crate) contents: Vec<u8>,
}

/// 遍历 `skills\` 下一级每个内置技能（目录名升序；只认目录，根下散件不算技能）
pub(crate) fn skills() -> Vec<BuiltinSkill> {
    SKILLS
        .dirs()
        .map(|dir| BuiltinSkill {
            name: entry_name(dir.path()),
            files: collect_files(dir, ""),
        })
        .collect()
}

/// 递归收集目录里的文件：相对路径由父路径逐段拼出，分隔符统一为 `/`
fn collect_files(dir: &Dir<'_>, prefix: &str) -> Vec<BuiltinFile> {
    let mut files: Vec<BuiltinFile> = dir
        .files()
        .map(|file| BuiltinFile {
            path: format!("{prefix}{}", entry_name(file.path())),
            contents: file.contents().to_vec(),
        })
        .collect();
    for child in dir.dirs() {
        let nested = format!("{prefix}{}/", entry_name(child.path()));
        files.extend(collect_files(child, &nested));
    }
    files
}

/// 内嵌路径的末段名：内嵌路径相对内嵌根、以 `/` 分隔
fn entry_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use include_dir::{Dir, DirEntry, File};

    use super::*;

    /// 内嵌表的递归遍历：子目录文件的相对路径由父路径逐段拼出，分隔符统一为 `/`
    #[test]
    fn collect_files_flattens_nested_directories() {
        static ENTRIES: &[DirEntry<'static>] = &[DirEntry::Dir(Dir::new(
            "demo/references",
            &[DirEntry::File(File::new("demo/references/a.txt", b"ref-a"))],
        ))];
        static NESTED: Dir<'static> = Dir::new("demo", ENTRIES);

        let files = collect_files(&NESTED, "");
        assert_eq!(files.len(), 1, "嵌套目录里的文件应被收进同一张表");
        assert_eq!(files[0].path, "references/a.txt", "路径应相对技能根");
        assert_eq!(files[0].contents, b"ref-a".to_vec());
    }
}
