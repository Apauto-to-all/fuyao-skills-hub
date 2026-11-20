//! 入库的源发现：有序判定，不是枚举。
//! 源目录自身含 `SKILL.md` 即单技能并立即短路，不再看子目录；否则只检查
//! 直接子目录，含 `SKILL.md` 者即技能、不含者跳过；更深层一律不进入。
//! 扫描跳过 `.` 开头的目录与 `node_modules`，不提供开关。

use std::fs;
use std::path::{Path, PathBuf};

use super::{SkillError, invalid};

/// 发现到的一个技能：目录即身份，名字取目录名
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Found {
    /// 技能名（= 所在目录名）
    pub name: String,
    /// 技能目录的绝对路径
    pub dir: PathBuf,
}

/// 扫描时一律跳过的目录名：`.` 开头的是工具与版本库目录，`node_modules` 是依赖树
fn skipped(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules"
}

/// 目录是否是技能根：判据只有 `SKILL.md` 在场这一条
fn is_skill(dir: &Path) -> bool {
    dir.join("SKILL.md").is_file()
}

/// 按两层规则发现源目录里的技能，按名字排序。
/// 一个都没发现即报源不成立——「空目录」与「装错了文件夹」在这里是同一件事
pub(super) fn discover(src_dir: &Path) -> Result<Vec<Found>, SkillError> {
    let label = src_dir.display().to_string();
    // 第一条：源目录自身就是技能，立即短路，子目录里的东西不再看
    if is_skill(src_dir) {
        let name = src_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Ok(vec![Found {
            name,
            dir: src_dir.to_path_buf(),
        }]);
    }
    let entries =
        fs::read_dir(src_dir).map_err(|e| invalid(&label, format!("读不到源目录: {e}")))?;
    let mut found: Vec<Found> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if skipped(&name) || !is_skill(&dir) {
            continue;
        }
        found.push(Found { name, dir });
    }
    if found.is_empty() {
        return Err(invalid(
            &label,
            "源目录既不是技能，也不含带 SKILL.md 的直接子目录".into(),
        ));
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    /// 目录树按「相对路径 → 是否技能」搭：路径逐段建目录，技能位放 SKILL.md
    fn tree(root: &Path, entries: &[(&str, bool)]) {
        for (path, is_skill) in entries {
            let dir = root.join(path);
            fs::create_dir_all(&dir).unwrap();
            if *is_skill {
                fs::write(
                    dir.join("SKILL.md"),
                    "---\nname: x\ndescription: 占位\n---\n",
                )
                .unwrap();
            }
        }
    }

    /// 源目录自身是技能：单技能，立即短路，子目录里的技能不被发现
    #[test]
    fn a_skill_directory_is_one_skill_and_stops_there() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("pack");
        tree(&src, &[("", true), ("inner", true)]);

        let found = discover(&src).unwrap();
        assert_eq!(
            found,
            vec![Found {
                name: "pack".into(),
                dir: src.clone()
            }],
            "自身是技能就只看它自己"
        );
    }

    /// 两层布局：直接子目录里的技能各算一个，不含 SKILL.md 的子目录跳过
    #[test]
    fn direct_children_are_the_skills() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path();
        tree(
            src,
            &[
                ("tdd", true),
                ("db", true),
                ("docs", false),
                ("notes", false),
            ],
        );

        let names: Vec<String> = discover(src).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["db", "tdd"], "按名字排序，非技能目录不进清单");
    }

    /// 三层布局：孙目录里的 SKILL.md 不被发现，更深层一律不进入
    #[test]
    fn grandchildren_are_never_entered() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path();
        tree(src, &[("outer/inner/deep", true), ("plain", true)]);

        let names: Vec<String> = discover(src).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["plain"], "孙目录的技能不入库");
    }

    /// `.` 开头的目录与 `node_modules` 一律跳过，不给开关
    #[test]
    fn dot_directories_and_node_modules_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path();
        tree(
            src,
            &[
                (".git/hooks", true),
                (".cache", true),
                ("node_modules/dep", true),
                ("real", true),
            ],
        );

        let names: Vec<String> = discover(src).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["real"]);
    }

    /// 一个技能都没发现：源不成立，报错点名源目录
    #[test]
    fn empty_source_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path();
        tree(src, &[("docs", false)]);

        let err = discover(src).unwrap_err();
        assert_eq!(err.error_code(), "invalid_source");
        assert!(err.to_string().contains("SKILL.md"), "{err}");
    }
}
