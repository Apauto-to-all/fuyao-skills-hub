//! hub 装配：一条龙初始化（目录树 + .gitignore + hub.toml + git 仓库 + 首提交）
//! 与 hub.toml 的读写。初始化幂等——对已初始化的 hub 拒绝重做且零破坏。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::git;
use crate::paths::HubPaths;
use thiserror::Error;

/// hub 装配错误
#[derive(Debug, Error)]
pub enum HubError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// hub.toml 解析失败
    #[error("hub.toml 解析失败: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// hub.toml 序列化失败
    #[error("hub.toml 序列化失败: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// 目标已是初始化过的 hub
    #[error("目录已是 hub: {0}")]
    AlreadyHub(String),
}

/// 提交身份（写入每次 commit）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserIdentity {
    /// 提交者名
    pub name: String,
    /// 提交者邮箱
    pub email: String,
}

impl Default for UserIdentity {
    fn default() -> Self {
        Self {
            name: "fuyao".to_string(),
            email: "fuyao@local".to_string(),
        }
    }
}

/// GC 策略配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GcConfig {
    /// 物化目录回收宽限天数（0 = 即时清）
    // TODO: M0 的 apply 顺手清按默认 0 即时清不读此值；宽限窗口由
    // 全量 gc（运维票）接线——届时先列清单、过宽限期再删
    #[serde(default)]
    pub grace_days: u32,
}

/// hub.toml 首版字段：身份 + GC 策略；remote 等留待后续版本
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct HubConfig {
    /// 配置结构版本
    pub schema_version: u32,
    /// 提交身份
    #[serde(default)]
    pub user: UserIdentity,
    /// GC 策略
    #[serde(default)]
    pub gc: GcConfig,
}

impl HubConfig {
    /// 首版默认配置
    pub fn v1() -> Self {
        Self {
            schema_version: 1,
            user: UserIdentity::default(),
            gc: GcConfig::default(),
        }
    }

    /// 序列化为 hub.toml 文本
    pub fn to_toml(&self) -> Result<String, HubError> {
        Ok(toml::to_string(self)?)
    }

    /// 从 hub.toml 文本解析
    pub fn from_toml(text: &str) -> Result<Self, HubError> {
        Ok(toml::from_str(text)?)
    }
}

/// hub.toml 两行关键忽略规则：物化目录（store/<skill>/<ver>/）与 cache/
pub const GITIGNORE_CONTENT: &str = "store/*/*/\ncache/\n";

/// 一条龙初始化 hub：目录树 → .gitignore → 默认 hub.toml → git 仓库 →
/// 以 hub.toml 身份首次提交。data\ 下已存在 .git 时拒绝且不触碰任何数据。
// TODO: M1 与全部写操作共用进程级文件锁
pub fn hub_init(install_dir: &Path) -> Result<HubPaths, HubError> {
    let paths = HubPaths::new(install_dir);
    // 幂等闸门：已是 hub 则拒绝，且本函数在此之前不产生任何磁盘副作用
    if paths.root().join(".git").exists() {
        return Err(HubError::AlreadyHub(paths.root().display().to_string()));
    }
    for dir in [
        paths.store(),
        paths.meta(),
        paths.proposals(),
        paths.cache(),
    ] {
        fs::create_dir_all(dir)?;
    }
    fs::write(paths.gitignore(), GITIGNORE_CONTENT)?;
    let cfg = HubConfig::v1();
    fs::write(paths.hub_toml(), cfg.to_toml()?)?;
    let repo = git::init(paths.root())?;
    git::commit_all(&repo, "init hub", &cfg.user.name, &cfg.user.email)?;
    Ok(paths)
}

/// 读取 hub.toml；文件缺失时返回 None（由调用方决定是否引导初始化）
pub fn load_config(paths: &HubPaths) -> Result<Option<HubConfig>, HubError> {
    match fs::read_to_string(paths.hub_toml()) {
        Ok(text) => Ok(Some(HubConfig::from_toml(&text)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 把配置写回 hub.toml
pub fn save_config(paths: &HubPaths, cfg: &HubConfig) -> Result<(), HubError> {
    fs::write(paths.hub_toml(), cfg.to_toml()?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 断言一条龙初始化后的完整落盘状态
    fn assert_initialized(paths: &HubPaths) {
        // 目录树齐全
        for dir in [
            paths.store(),
            paths.meta(),
            paths.proposals(),
            paths.cache(),
        ] {
            assert!(dir.is_dir(), "缺少目录: {dir:?}");
        }
        // .gitignore 两行关键规则
        let gi = fs::read_to_string(paths.gitignore()).unwrap();
        assert_eq!(gi, "store/*/*/\ncache/\n");
        // hub.toml 可读回且是首版默认值
        let cfg = load_config(paths).unwrap().expect("hub.toml 应存在");
        assert_eq!(cfg, HubConfig::v1());
        // git 仓库存在、main 分支、关 autocrlf、工作区干净（首提交已完成）
        let repo = git2::Repository::open(paths.root()).unwrap();
        let head = repo.find_reference("HEAD").unwrap();
        assert_eq!(
            head.symbolic_target().unwrap(),
            Some("refs/heads/main"),
            "初始分支应为 main"
        );
        assert_eq!(
            repo.config().unwrap().get_string("core.autocrlf").unwrap(),
            "false"
        );
        assert!(!git::is_dirty(&repo).unwrap(), "首提交后工作区应干净");
        // 首提交身份来自 hub.toml
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(commit.author().name().unwrap(), "fuyao");
        assert_eq!(commit.author().email().unwrap(), "fuyao@local");
    }

    #[test]
    fn hub_init_creates_full_layout_and_first_commit() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub_init(install.path()).unwrap();
        assert_initialized(&paths);
    }

    #[test]
    fn hub_init_rejects_existing_hub_without_damage() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub_init(install.path()).unwrap();
        // 人为改动 hub.toml 后再次 init：拒绝且原数据不动
        let cfg = load_config(&paths).unwrap().unwrap();
        let mut modified = cfg.clone();
        modified.user.name = "someone".to_string();
        save_config(&paths, &modified).unwrap();
        assert!(matches!(
            hub_init(install.path()),
            Err(HubError::AlreadyHub(_))
        ));
        let reloaded = load_config(&paths).unwrap().unwrap();
        assert_eq!(reloaded.user.name, "someone", "重复 init 不得破坏现有数据");
    }

    #[test]
    fn load_config_returns_none_when_missing() {
        let install = tempfile::tempdir().unwrap();
        let paths = HubPaths::new(install.path());
        assert!(load_config(&paths).unwrap().is_none());
    }

    #[test]
    fn config_toml_roundtrip_keeps_all_fields() {
        let mut cfg = HubConfig::v1();
        cfg.user.name = "alice".to_string();
        cfg.gc.grace_days = 7;
        let text = cfg.to_toml().unwrap();
        let back = HubConfig::from_toml(&text).unwrap();
        assert_eq!(back, cfg);
        // 缺省字段（无 user/gc 段）落默认值
        let minimal = HubConfig::from_toml("schema_version = 1\n").unwrap();
        assert_eq!(minimal.user, UserIdentity::default());
        assert_eq!(minimal.gc.grace_days, 0);
    }
}
