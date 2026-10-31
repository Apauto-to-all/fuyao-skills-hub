//! hub 目录布局单一事实源：全库只有这里知道「路径怎么拼」。
//! 布局规则（portable）：安装目录下的 data\ 即 hub 根，内含
//! store\（技能工作副本与物化目录）、meta\、proposals\、cache\。

use std::path::{Path, PathBuf};

/// hub 路径族：由安装目录一次性派生全部子路径
#[derive(Debug, Clone)]
pub struct HubPaths {
    /// hub 根（= 安装目录\data\），git 仓库就建在这里
    root: PathBuf,
}

impl HubPaths {
    /// 从安装目录定位 hub：唯一规则是「自己所在目录下的 data\」
    pub fn new(install_dir: &Path) -> Self {
        Self {
            root: install_dir.join("data"),
        }
    }

    /// 已知 hub 根时直接构造（gc / backup / doctor 等以 hub 根为入口的路径族）
    pub fn from_root(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// 解析 hub 位置：显式指定的根目录优先，缺省兜底为当前
    /// 可执行文件所在目录下的 data\（portable 布局，不读环境变量）
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

    /// 技能存储区（tracked 工作副本 = 编辑面；版本子目录 = 物化面）
    pub fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    /// 技能元数据区（每技能一份 toml）
    pub fn meta(&self) -> PathBuf {
        self.root.join("meta")
    }

    /// 进化提案历史区
    pub fn proposals(&self) -> PathBuf {
        self.root.join("proposals")
    }

    /// 本地缓存区（可整体删除重建）
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// hub 配置文件
    pub fn hub_toml(&self) -> PathBuf {
        self.root.join("hub.toml")
    }

    /// hub 仓库 .gitignore
    pub fn gitignore(&self) -> PathBuf {
        self.root.join(".gitignore")
    }

    /// 技能 tracked 工作副本目录（编辑面，可脏）
    pub fn skill_workdir(&self, skill: &str) -> PathBuf {
        self.store().join(skill)
    }

    /// 技能某版本的物化目录（部署面，ignored，可删可重建）
    pub fn materialized(&self, skill: &str, version: &str) -> PathBuf {
        self.store().join(skill).join(version)
    }

    /// 项目侧中立链接落点：链接一律铺在 <project>\.fsh\skills\<skill>，
    /// 目标工具目录的映射归 adapter 阶段处理
    pub fn project_link_dst(project_dir: &Path, skill: &str) -> PathBuf {
        project_dir.join(".fsh").join("skills").join(skill)
    }

    /// 项目侧人写声明文件（要哪些技能、版本、目标工具）
    pub fn project_manifest(project_dir: &Path) -> PathBuf {
        project_dir.join("skills.toml")
    }

    /// 项目侧机器写账本文件（精确版本 + hash，提交 git）
    pub fn project_lock(project_dir: &Path) -> PathBuf {
        project_dir.join("skills.lock.toml")
    }

    /// 项目侧本机状态文件（实际链接与时间戳，不提交）
    pub fn project_state(project_dir: &Path) -> PathBuf {
        project_dir.join("state.toml")
    }

    /// hub 侧项目登记簿（apply 登记，gc 与 doctor 的引用查找账本）
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
    fn derives_all_paths_from_install_dir() {
        let p = HubPaths::new(Path::new("E:/fuyao"));
        assert_eq!(p.root(), Path::new("E:/fuyao/data"));
        assert_eq!(p.store(), Path::new("E:/fuyao/data/store"));
        assert_eq!(p.meta(), Path::new("E:/fuyao/data/meta"));
        assert_eq!(p.proposals(), Path::new("E:/fuyao/data/proposals"));
        assert_eq!(p.cache(), Path::new("E:/fuyao/data/cache"));
        assert_eq!(p.hub_toml(), Path::new("E:/fuyao/data/hub.toml"));
        assert_eq!(p.gitignore(), Path::new("E:/fuyao/data/.gitignore"));
    }

    #[test]
    fn derives_skill_and_project_paths() {
        let p = HubPaths::new(Path::new("E:/fuyao"));
        assert_eq!(
            p.skill_workdir("demo"),
            Path::new("E:/fuyao/data/store/demo")
        );
        assert_eq!(
            p.materialized("demo", "1.0.0"),
            Path::new("E:/fuyao/data/store/demo/1.0.0")
        );
        assert_eq!(
            HubPaths::project_link_dst(Path::new("P:/proj"), "demo"),
            Path::new("P:/proj/.fsh/skills/demo")
        );
        assert_eq!(
            HubPaths::project_manifest(Path::new("P:/proj")),
            Path::new("P:/proj/skills.toml")
        );
        assert_eq!(
            HubPaths::project_lock(Path::new("P:/proj")),
            Path::new("P:/proj/skills.lock.toml")
        );
        assert_eq!(
            HubPaths::project_state(Path::new("P:/proj")),
            Path::new("P:/proj/state.toml")
        );
        assert_eq!(p.projects_toml(), Path::new("E:/fuyao/data/projects.toml"));
    }
}
