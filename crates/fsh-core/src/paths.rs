//! hub 目录布局单一事实源：全库只有这里知道「路径怎么拼」。
//! 定位链（CLI 与 GUI 共用的唯一规则）：显式根 > 环境变量 FSH_HUB >
//! 缺省家目录点目录 ~/.fuyao-skills-hub。hub 根内含
//! store\（技能工作副本，唯一元数据住所）、archive\（归档面，与 store 同构）、
//! agents\（agent 配置覆盖）。

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::ErrorCode;

/// hub 位置环境变量：全库唯一的读点在本模块
pub const FSH_HUB_ENV: &str = "FSH_HUB";

/// 缺省 hub 根目录名（家目录下点目录，跨平台同形）
const DEFAULT_HUB_DIR: &str = ".fuyao-skills-hub";

/// hub 根下的 tracked 面名：仓库布局的唯一来源。
/// 顺序即 `.gitignore` 白名单的书写顺序，新增面只改这一处
pub const FACES: &[&str] = &[AGENTS_FACE, STORE_FACE, ARCHIVE_FACE];

/// agent 配置覆盖目录的面名
pub const AGENTS_FACE: &str = "agents";

/// 技能编辑面的面名
pub const STORE_FACE: &str = "store";

/// 技能归档面的面名
pub const ARCHIVE_FACE: &str = "archive";

/// 存放技能的面名：只有这两个面里的目录才构成技能
pub const SKILL_FACES: &[&str] = &[STORE_FACE, ARCHIVE_FACE];

/// hub 定位错误
#[derive(Debug, Error)]
pub enum PathsError {
    /// 家目录解析失败且无任何显式覆盖：宁可启动失败也不把数据写进错误位置
    #[error("无法定位家目录，缺省 hub 不可用；设置环境变量 FSH_HUB 或用 --hub 指定 hub 根")]
    NoHome,
}

impl ErrorCode for PathsError {
    fn error_code(&self) -> &'static str {
        "no_home_dir"
    }
}

/// 规范成绝对路径：相对路径按进程当前目录展开。
/// hub 根必须绝对：git2 的 checkout 目标按仓库工作目录解析，相对路径会被再拼一次。
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// hub 路径族：由 hub 根一次性派生全部子路径
#[derive(Debug, Clone)]
pub struct HubPaths {
    /// hub 根，git 仓库就建在这里
    root: PathBuf,
}

impl HubPaths {
    /// 已知 hub 根时直接构造（init / gc / doctor 等以 hub 根为入口的路径族）
    pub fn from_root(root: &Path) -> Self {
        Self {
            root: absolute(root),
        }
    }

    /// 解析 hub 位置：定位链的唯一入口——显式根 > 环境变量 FSH_HUB >
    /// 缺省 ~/.fuyao-skills-hub。空字符串环境变量视为未设置；
    /// 家目录解析失败报错，不做静默降级
    pub fn discover(hub_opt: Option<&Path>) -> Result<Self, PathsError> {
        Self::discover_from(
            hub_opt,
            std::env::var(FSH_HUB_ENV).ok().as_deref(),
            std::env::home_dir(),
        )
    }

    /// 定位链的纯函数形态：环境变量原文与家目录由调用方注入，测试不碰真实环境
    pub(crate) fn discover_from(
        hub_opt: Option<&Path>,
        env_hub: Option<&str>,
        home: Option<PathBuf>,
    ) -> Result<Self, PathsError> {
        if let Some(root) = hub_opt {
            return Ok(Self::from_root(root));
        }
        if let Some(root) = env_hub.filter(|v| !v.is_empty()) {
            return Ok(Self::from_root(Path::new(root)));
        }
        match home {
            Some(home) => Ok(Self::from_root(&home.join(DEFAULT_HUB_DIR))),
            None => Err(PathsError::NoHome),
        }
    }

    /// hub 根目录
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 技能存储区（tracked 工作副本 = 编辑面，键为 store 内相对路径）
    pub fn store(&self) -> PathBuf {
        self.root.join(STORE_FACE)
    }

    /// 归档面根目录（tracked，储存结构与 store 完全一致）
    pub fn archive(&self) -> PathBuf {
        self.root.join(ARCHIVE_FACE)
    }

    /// agent 配置覆盖目录（内置名单在代码里，同名 TOML 覆盖、新文件名新增）
    pub fn agents(&self) -> PathBuf {
        self.root.join("agents")
    }

    /// 全部 tracked 面的根目录，按 [`FACES`] 的顺序给出。
    /// 装配与修复建目录时从这里取，新增面不必再改调用方
    pub fn face_dirs(&self) -> Vec<PathBuf> {
        FACES.iter().map(|face| self.root.join(face)).collect()
    }

    /// hub 配置文件
    pub fn hub_toml(&self) -> PathBuf {
        self.root.join("hub.toml")
    }

    /// 日志目录（ignored，与 hub 同生共死；排除在仓库之外靠 `.gitignore` 里的 `logs/`）
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// hub 仓库 .gitignore
    pub fn gitignore(&self) -> PathBuf {
        self.root.join(".gitignore")
    }

    /// 技能 tracked 工作副本目录（编辑面，可脏）。键是 store 内相对路径，
    /// 可多级（如 mattpocock/skills/tdd），join 直接铺出嵌套目录
    pub fn skill_workdir(&self, key: &str) -> PathBuf {
        self.store().join(key)
    }

    /// 技能的归档副本目录（archive\<键>，与工作副本同构）
    pub fn skill_archived(&self, key: &str) -> PathBuf {
        self.archive().join(key)
    }

    /// hub 侧项目登记簿（用法的唯一记录：项目 → 技能 → 版本 / agent / 链接方式）
    pub fn projects_toml(&self) -> PathBuf {
        self.root.join("projects.toml")
    }

    /// 装配完整性：初始化应产出的三件东西，缺哪件报哪件。
    /// 只判存在、不读内容——判据必须不可失败，内容完好性归 doctor
    pub fn missing_parts(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !self.root.join(".git").exists() {
            missing.push(".git");
        }
        if !self.gitignore().exists() {
            missing.push(".gitignore");
        }
        if !self.hub_toml().exists() {
            missing.push("hub.toml");
        }
        missing
    }

    /// hub 是否完整装配过：全库唯一的初始化判据
    pub fn is_initialized(&self) -> bool {
        self.missing_parts().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_root_wins_over_env_and_home() {
        let p = HubPaths::discover_from(
            Some(Path::new("E:/x/mydata")),
            Some("E:/from-env"),
            Some(PathBuf::from("E:/home")),
        )
        .unwrap();
        assert_eq!(p.root(), Path::new("E:/x/mydata"));
    }

    #[test]
    fn env_hub_wins_over_home_default() {
        let p = HubPaths::discover_from(None, Some("E:/from-env"), Some(PathBuf::from("E:/home")))
            .unwrap();
        assert_eq!(p.root(), Path::new("E:/from-env"));
    }

    #[test]
    fn empty_env_hub_falls_back_to_home_default() {
        let p = HubPaths::discover_from(None, Some(""), Some(PathBuf::from("E:/home"))).unwrap();
        assert_eq!(
            p.root(),
            Path::new("E:/home/.fuyao-skills-hub"),
            "空字符串环境变量应视为未设置"
        );
    }

    #[test]
    fn default_root_is_dot_dir_under_home() {
        let p = HubPaths::discover_from(None, None, Some(PathBuf::from("E:/home"))).unwrap();
        assert_eq!(p.root(), Path::new("E:/home/.fuyao-skills-hub"));
    }

    #[test]
    fn missing_home_without_overrides_is_hard_error() {
        let err = HubPaths::discover_from(None, None, None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("FSH_HUB"), "报错要给出环境变量指引: {msg}");
        assert!(msg.contains("--hub"), "报错要给出参数指引: {msg}");
    }

    #[test]
    fn relative_input_is_normalized_to_absolute_root() {
        // 相对 hub 根必须归一成绝对：git2 的 checkout 目标按仓库目录解析，
        // 相对路径会被再拼一层
        let p = HubPaths::from_root(Path::new("relative/data"));
        assert!(p.root().is_absolute(), "root 应为绝对路径: {:?}", p.root());
    }

    #[test]
    fn derives_all_paths_from_root() {
        let p = HubPaths::from_root(Path::new("E:/fuyao-skills-hub"));
        assert_eq!(p.root(), Path::new("E:/fuyao-skills-hub"));
        assert_eq!(p.store(), Path::new("E:/fuyao-skills-hub/store"));
        assert_eq!(p.archive(), Path::new("E:/fuyao-skills-hub/archive"));
        assert_eq!(p.agents(), Path::new("E:/fuyao-skills-hub/agents"));
        assert_eq!(p.hub_toml(), Path::new("E:/fuyao-skills-hub/hub.toml"));
        assert_eq!(p.gitignore(), Path::new("E:/fuyao-skills-hub/.gitignore"));
        assert_eq!(
            p.projects_toml(),
            Path::new("E:/fuyao-skills-hub/projects.toml")
        );
    }

    #[test]
    fn initialized_requires_all_three_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let p = HubPaths::from_root(dir.path());
        assert_eq!(p.missing_parts(), vec![".git", ".gitignore", "hub.toml"]);
        assert!(!p.is_initialized());

        std::fs::create_dir_all(p.root().join(".git")).unwrap();
        assert!(!p.is_initialized(), "只有仓库不算装配完整");
        std::fs::write(p.gitignore(), "").unwrap();
        assert!(!p.is_initialized(), "缺 hub.toml 仍不算完整");
        std::fs::write(p.hub_toml(), "").unwrap();
        assert!(p.is_initialized(), "三件齐全才算初始化完成");
    }

    #[test]
    fn derives_skill_and_archive_paths() {
        let p = HubPaths::from_root(Path::new("E:/fuyao-skills-hub"));
        assert_eq!(
            p.skill_workdir("demo"),
            Path::new("E:/fuyao-skills-hub/store/demo")
        );
        assert_eq!(
            p.skill_archived("demo"),
            Path::new("E:/fuyao-skills-hub/archive/demo")
        );
        assert_eq!(
            p.skill_workdir("mattpocock/skills/tdd"),
            Path::new("E:/fuyao-skills-hub/store/mattpocock/skills/tdd")
        );
        assert_eq!(
            p.skill_archived("mattpocock/skills/tdd"),
            Path::new("E:/fuyao-skills-hub/archive/mattpocock/skills/tdd")
        );
    }
}
