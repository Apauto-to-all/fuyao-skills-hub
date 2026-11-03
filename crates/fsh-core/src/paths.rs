//! hub 目录布局单一事实源：全库只有这里知道「路径怎么拼」。
//! 布局规则（portable）：安装目录下的 data\\ 即 hub 根，内含
//! store\\（技能工作副本，唯一元数据住所）、export\\（按需导出目录）、
//! agents\\（agent 配置覆盖）。

use std::path::{Path, PathBuf};

/// 规范成绝对路径：相对路径按进程当前目录展开。
/// hub 根必须绝对：git2 的 checkout 目标按仓库工作目录解析，相对路径会被再拼一次。
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// hub 路径族：由安装目录一次性派生全部子路径
#[derive(Debug, Clone)]
pub struct HubPaths {
    /// hub 根（= 安装目录\\data\\），git 仓库就建在这里
    root: PathBuf,
}

impl HubPaths {
    /// 从安装目录定位 hub：唯一规则是「自己所在目录下的 data\\」
    pub fn new(install_dir: &Path) -> Self {
        Self {
            root: absolute(&install_dir.join("data")),
        }
    }

    /// 已知 hub 根时直接构造（gc / backup / doctor 等以 hub 根为入口的路径族）
    pub fn from_root(root: &Path) -> Self {
        Self {
            root: absolute(root),
        }
    }

    /// 安装目录（hub 根的父目录；安装目录 ↔ hub 根的换算只发生在本模块）
    pub fn install(&self) -> &Path {
        self.root.parent().unwrap_or(Path::new("."))
    }

    /// 解析 hub 位置：显式指定的根目录优先，缺省兜底为当前
    /// 可执行文件所在目录下的 data\\（portable 布局，不读环境变量）
    pub fn discover(hub_opt: Option<&Path>) -> Self {
        match hub_opt {
            Some(root) => Self::from_root(root),
            None => {
                // current_exe 失败极罕见（进程文件被移动等），退化为当前目录
                let exe = std::env::current_exe()
                    .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
                let install = exe.parent().unwrap_or(Path::new("."));
                Self::new(install)
            }
        }
    }

    /// hub 根目录
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 技能存储区（tracked 工作副本 = 编辑面，键为 store 内相对路径）
    pub fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    /// 导出目录根（ignored，可整体删除重建）
    pub fn export(&self) -> PathBuf {
        self.root.join("export")
    }

    /// agent 配置覆盖目录（内置名单在代码里，同名 TOML 覆盖、新文件名新增）
    pub fn agents(&self) -> PathBuf {
        self.root.join("agents")
    }

    /// hub 配置文件
    pub fn hub_toml(&self) -> PathBuf {
        self.root.join("hub.toml")
    }

    /// hub 仓库 .gitignore
    pub fn gitignore(&self) -> PathBuf {
        self.root.join(".gitignore")
    }

    /// 技能 tracked 工作副本目录（编辑面，可脏）。键是 store 内相对路径，
    /// 可多级（如 anthropics/skills/pdf），join 直接铺出嵌套目录
    pub fn skill_workdir(&self, key: &str) -> PathBuf {
        self.store().join(key)
    }

    /// 技能某版本的导出目录（固定版本用）。住在 export\\ 下：
    /// export 整体被 git 忽略，任意深度键的导出目录天然不需要忽略规则
    pub fn exported(&self, key: &str, version: &str) -> PathBuf {
        self.export().join(key).join(version)
    }

    /// hub 侧项目登记簿（用法的唯一记录：项目 → 技能 → 版本 / agent / 链接方式）
    pub fn projects_toml(&self) -> PathBuf {
        self.root.join("projects.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_explicit_root_over_fallback() {
        let p = HubPaths::discover(Some(Path::new("E:/x/mydata")));
        assert_eq!(p.root(), Path::new("E:/x/mydata"));
    }

    #[test]
    fn relative_input_is_normalized_to_absolute_root() {
        // 相对 hub 根必须归一成绝对：git2 的 checkout 目标按仓库目录解析，
        // 相对路径会被再拼一层
        let p = HubPaths::from_root(Path::new("relative/data"));
        assert!(p.root().is_absolute(), "root 应为绝对路径: {:?}", p.root());
    }

    #[test]
    fn install_dir_round_trips_through_root() {
        let p = HubPaths::new(Path::new("E:/fuyao"));
        assert_eq!(
            HubPaths::from_root(p.root()).install(),
            Path::new("E:/fuyao")
        );
    }

    #[test]
    fn derives_all_paths_from_install_dir() {
        let p = HubPaths::new(Path::new("E:/fuyao"));
        assert_eq!(p.root(), Path::new("E:/fuyao/data"));
        assert_eq!(p.store(), Path::new("E:/fuyao/data/store"));
        assert_eq!(p.export(), Path::new("E:/fuyao/data/export"));
        assert_eq!(p.agents(), Path::new("E:/fuyao/data/agents"));
        assert_eq!(p.hub_toml(), Path::new("E:/fuyao/data/hub.toml"));
        assert_eq!(p.gitignore(), Path::new("E:/fuyao/data/.gitignore"));
    }

    #[test]
    fn derives_skill_and_export_paths() {
        let p = HubPaths::new(Path::new("E:/fuyao"));
        assert_eq!(
            p.skill_workdir("demo"),
            Path::new("E:/fuyao/data/store/demo")
        );
        assert_eq!(
            p.exported("demo", "1.0.0"),
            Path::new("E:/fuyao/data/export/demo/1.0.0")
        );
        // 多级键：工作副本嵌套铺开，导出目录跟到 export 下同构嵌套
        assert_eq!(
            p.skill_workdir("anthropics/skills/pdf"),
            Path::new("E:/fuyao/data/store/anthropics/skills/pdf")
        );
        assert_eq!(
            p.exported("anthropics/skills/pdf", "1.2.0"),
            Path::new("E:/fuyao/data/export/anthropics/skills/pdf/1.2.0")
        );
        assert_eq!(p.projects_toml(), Path::new("E:/fuyao/data/projects.toml"));
    }
}
