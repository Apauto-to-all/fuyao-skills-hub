//! hub 装配面：一条龙初始化、hub.toml 配置读写与项目登记簿。
//! 本模块为目录模块：装配（init）、配置（config）、登记簿（registry）
//! 各自独立，此处只留错误类型、共用小件与对外重导出，调用方路径不变。

use crate::ErrorCode;
use crate::git;
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
    /// 进程级文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] crate::lock::LockError),
}

impl ErrorCode for HubError {
    fn error_code(&self) -> &'static str {
        match self {
            HubError::Io(_) => "io",
            HubError::Git(g) => g.error_code(),
            HubError::TomlDe(_) => "toml_parse",
            HubError::TomlSer(_) => "toml_serialize",
            HubError::AlreadyHub(_) => "already_hub",
            HubError::Lock(l) => l.error_code(),
        }
    }
}

/// hub.toml 关键忽略规则：导出目录住 export\（整体忽略，与键深度无关）、
/// 项目登记簿（含本机绝对路径，不入版本真身）。store\ 与 agents\ 全部 tracked
pub const GITIGNORE_CONTENT: &str = "export/\nprojects.toml\n";

/// 当前 Unix 秒（登记时间戳的统一来源）
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

mod config;
mod init;
mod registry;

pub use config::{GcConfig, HubConfig, UserIdentity, load_config, save_config};
pub use init::{InitOutcome, hub_init, init};
pub use registry::{ProjectRecord, ProjectRegistry, ProjectSkill, load_registry, save_registry};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    use crate::paths::HubPaths;

    /// 断言一条龙初始化后的完整落盘状态
    fn assert_initialized(paths: &HubPaths) {
        // 目录树齐全（元数据唯一住所在内容树里）
        for dir in [paths.store(), paths.export(), paths.agents()] {
            assert!(dir.is_dir(), "缺少目录: {dir:?}");
        }
        assert!(
            !paths.root().join("meta").exists(),
            "hub 布局里没有 meta 目录"
        );
        // .gitignore 两行关键规则
        let gi = fs::read_to_string(paths.gitignore()).unwrap();
        assert_eq!(gi, "export/\nprojects.toml\n");
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
        // 全局写锁落在 export/ 根下，且被 git 忽略：锁文件永远不进任何提交
        let lock = paths.export().join(".lock");
        assert!(lock.is_file(), "缺少全局写锁文件: {lock:?}");
        assert!(
            repo.status_should_ignore(std::path::Path::new("export/.lock"))
                .unwrap(),
            "export/.lock 必须被 git 忽略"
        );
        // 首提交身份来自 hub.toml
        let commit = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(commit.author().name().unwrap(), "fuyao-skills-hub");
        assert_eq!(commit.author().email().unwrap(), "fuyao-skills-hub@local");
    }

    #[test]
    fn hub_init_creates_full_layout_and_first_commit() {
        let install = tempfile::tempdir().unwrap();
        let paths = hub_init(install.path()).unwrap();
        assert_initialized(&paths);
    }

    #[test]
    fn init_first_run_initializes_hub() {
        let install = tempfile::tempdir().unwrap();
        let outcome = init(install.path(), false).unwrap();
        assert!(outcome.initialized, "首建应报告 initialized");
        assert!(!outcome.already);
        assert_initialized(&HubPaths::new(install.path()));
    }

    #[test]
    fn init_rerun_is_idempotent_and_touches_nothing() {
        let install = tempfile::tempdir().unwrap();
        init(install.path(), false).unwrap();
        let paths = HubPaths::new(install.path());
        let before = fs::read_to_string(paths.hub_toml()).unwrap();
        let outcome = init(install.path(), false).unwrap();
        assert!(
            !outcome.initialized && outcome.already,
            "幂等重跑应只报告 already"
        );
        assert_eq!(
            fs::read_to_string(paths.hub_toml()).unwrap(),
            before,
            "幂等重跑不得改动现有数据"
        );
    }

    #[test]
    fn init_dry_run_lists_actions_and_writes_nothing() {
        let install = tempfile::tempdir().unwrap();
        let outcome = init(install.path(), true).unwrap();
        assert!(outcome.dry_run && !outcome.already);
        assert_eq!(
            outcome.actions,
            vec![
                "create store/ export/ agents/",
                "write .gitignore hub.toml",
                "git init + first commit",
            ]
        );
        assert_eq!(
            fs::read_dir(install.path()).unwrap().count(),
            0,
            "预演不得落盘"
        );
    }

    #[test]
    fn init_dry_run_on_initialized_hub_reports_already() {
        let install = tempfile::tempdir().unwrap();
        init(install.path(), false).unwrap();
        let outcome = init(install.path(), true).unwrap();
        assert!(
            outcome.dry_run && outcome.already,
            "已建的预演应报告 already"
        );
        assert!(outcome.actions.is_empty());
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

    #[test]
    fn save_registry_accepts_held_token() {
        let install = tempfile::tempdir().unwrap();
        let paths = HubPaths::new(install.path());
        // 凭证随取随用：持锁写登记簿成功，语句结束凭证即释放
        let reg = ProjectRegistry::default();
        save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        assert_eq!(load_registry(&paths).unwrap(), ProjectRegistry::default());
    }

    #[test]
    fn registry_roundtrip_and_upsert() {
        let install = tempfile::tempdir().unwrap();
        let paths = HubPaths::new(install.path());
        // 缺失文件 = 空登记簿
        assert_eq!(load_registry(&paths).unwrap(), ProjectRegistry::default());

        let mut reg = ProjectRegistry::default();
        reg.upsert(install.path(), 100);
        reg.upsert(install.path(), 200); // 同路径 upsert 只刷新时间戳
        save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        let back = load_registry(&paths).unwrap();
        assert_eq!(back.projects.len(), 1);
        assert_eq!(back.projects[0].last_used, 200);
    }

    /// 引用判定只读登记簿：技能列表里出现键即算被引用
    #[test]
    fn referencing_projects_reads_skill_list() {
        let mut reg = ProjectRegistry::default();
        reg.upsert(Path::new("P:/a"), 1);
        reg.upsert(Path::new("P:/b"), 2);
        reg.projects[0].skills.push(ProjectSkill {
            key: "demo".into(),
            version: None,
            agents: vec!["universal".into()],
            link: crate::link::LinkKind::Junction,
        });
        let refs = reg.referencing_projects("demo");
        assert_eq!(refs.len(), 1, "只列出真正引用的项目");
        assert!(refs[0].ends_with("a") || refs[0].ends_with("b"));
        assert!(reg.referencing_projects("other").is_empty());
    }
}
