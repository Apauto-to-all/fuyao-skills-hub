//! hub 装配：一条龙初始化（目录树 + .gitignore + hub.toml + git 仓库 + 首提交）
//! 与 hub.toml 的读写。初始化幂等——对已初始化的 hub 拒绝重做且零破坏。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ErrorCode;
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
    // 宽限窗口只对带时间戳的对象生效（登记簿死项目记录）；
    // 物化目录无「失引用时刻」可考，无引用即清，后悔药 = tag 重物化
    #[serde(default)]
    pub grace_days: u32,
}

/// 备份提醒配置
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BackupConfig {
    /// 超过 N 天未备份时 doctor 提醒（N 天 = 本值）
    #[serde(default = "default_remind_days")]
    pub remind_days: u32,
}

/// remind_days 缺省值（doctor 兜底读取配置失败时也用它，保持单一来源）
pub(crate) fn default_remind_days() -> u32 {
    7
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            remind_days: default_remind_days(),
        }
    }
}

/// hub.toml 首版字段：身份 + GC 策略 + 备份提醒；remote 等留待后续版本
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
    /// 备份提醒
    #[serde(default)]
    pub backup: BackupConfig,
}

impl HubConfig {
    /// 首版默认配置
    pub fn v1() -> Self {
        Self {
            schema_version: 1,
            user: UserIdentity::default(),
            gc: GcConfig::default(),
            backup: BackupConfig::default(),
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

/// hub.toml 关键忽略规则：物化目录（store/<skill>/<ver>/）、cache/ 与
/// 项目登记簿（含本机绝对路径，不入版本真身）
pub const GITIGNORE_CONTENT: &str = "store/*/*/\ncache/\nprojects.toml\n";

/// 一条龙初始化 hub：目录树 → .gitignore → 默认 hub.toml → git 仓库 →
/// 以 hub.toml 身份首次提交。data\ 下已存在 .git 时拒绝且不触碰任何数据。
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
    // 全局写锁：初始化也是写操作，与入库 / apply / gc 互斥
    let _token = crate::lock::write_lock(&paths)?;
    // 锁内复查闸门：并发第二个 init 在此拿到 AlreadyHub（幂等），而不是 git 报错
    if paths.root().join(".git").exists() {
        return Err(HubError::AlreadyHub(paths.root().display().to_string()));
    }
    fs::write(paths.gitignore(), GITIGNORE_CONTENT)?;
    let cfg = HubConfig::v1();
    fs::write(paths.hub_toml(), cfg.to_toml()?)?;
    let repo = git::init(paths.root())?;
    git::commit_all(&repo, "init hub", &cfg.user.name, &cfg.user.email)?;
    Ok(paths)
}

/// init 命令的结果载荷。四种形态经 serde 跳过空值后直出信封 data：
/// 首建 `{initialized}`、幂等重跑 `{already_initialized}`、
/// 未建的预演 `{dry_run, actions}`、已建的预演 `{dry_run, already_initialized}`
#[derive(Debug, Default, Serialize)]
pub struct InitOutcome {
    /// 预演报告标志（仅预演出现）
    #[serde(skip_serializing_if = "is_false")]
    pub dry_run: bool,
    /// 本次完成首次初始化（仅实跑首建出现）
    #[serde(skip_serializing_if = "is_false")]
    pub initialized: bool,
    /// 目标已是初始化过的 hub（实跑幂等重跑、已建的预演出现）
    #[serde(rename = "already_initialized", skip_serializing_if = "is_false")]
    pub already: bool,
    /// 预演动作清单（仅未建的预演出现）
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<String>,
}

/// serde 跳过空值的判定（false 字段不入信封）
fn is_false(b: &bool) -> bool {
    !*b
}

/// init 命令的完整语义入口：幂等判定与预演动作清单。
/// 实跑委托一条龙装配；已是 hub 属幂等重跑，不算错误。
pub fn init(install_dir: &Path, dry_run: bool) -> Result<InitOutcome, HubError> {
    let paths = HubPaths::new(install_dir);
    // 预演：零落盘，只判幂等与列动作
    if dry_run {
        if paths.root().join(".git").exists() {
            return Ok(InitOutcome {
                dry_run: true,
                already: true,
                ..InitOutcome::default()
            });
        }
        return Ok(InitOutcome {
            dry_run: true,
            actions: vec![
                "create store/ meta/ proposals/ cache/".to_string(),
                "write .gitignore hub.toml".to_string(),
                "git init + first commit".to_string(),
            ],
            ..InitOutcome::default()
        });
    }
    match hub_init(install_dir) {
        Ok(_) => Ok(InitOutcome {
            initialized: true,
            ..InitOutcome::default()
        }),
        // 幂等闸门命中：原样报告，不破坏任何数据
        Err(HubError::AlreadyHub(_)) => Ok(InitOutcome {
            already: true,
            ..InitOutcome::default()
        }),
        Err(e) => Err(e),
    }
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

// ---------- 项目登记簿（projects.toml，机器写） ----------

/// 登记簿：hub 侧全部项目名单 + 备份时间戳。
/// gc 与 doctor 的引用查找账本——遍历它读各项目 lock，不扫盘。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectRegistry {
    /// 最后一次成功备份时间（Unix 秒，0 = 从未备份）
    #[serde(default)]
    pub last_backup: u64,
    /// 全部已登记项目
    #[serde(default)]
    pub projects: Vec<ProjectRecord>,
}

/// 单条项目登记
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectRecord {
    /// 项目绝对路径
    pub path: String,
    /// 最后一次 apply 时间（Unix 秒）
    pub last_applied: u64,
}

impl ProjectRegistry {
    /// 登记或刷新一个项目（同路径 upsert，时间戳取传入值）
    pub fn upsert(&mut self, project_dir: &Path, now: u64) {
        let canon = project_dir
            .canonicalize()
            .unwrap_or_else(|_| project_dir.to_path_buf());
        let path = canon.display().to_string();
        match self.projects.iter_mut().find(|p| p.path == path) {
            Some(rec) => rec.last_applied = now,
            None => self.projects.push(ProjectRecord {
                path,
                last_applied: now,
            }),
        }
    }
}

/// 当前 Unix 秒（登记与备份时间戳的统一来源）
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 读取登记簿；文件缺失视为空登记簿（尚未 apply 过任何项目）
pub fn load_registry(paths: &HubPaths) -> Result<ProjectRegistry, HubError> {
    match fs::read_to_string(paths.projects_toml()) {
        Ok(text) => Ok(toml::from_str(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectRegistry::default()),
        Err(e) => Err(e.into()),
    }
}

/// 写回登记簿；凭证参数保证调用方已持全局写锁（先锁后写）
pub fn save_registry(
    paths: &HubPaths,
    registry: &ProjectRegistry,
    _token: &crate::lock::WriteToken,
) -> Result<(), HubError> {
    if let Some(parent) = paths.projects_toml().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(paths.projects_toml(), toml::to_string(registry)?)?;
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
        assert_eq!(gi, "store/*/*/\ncache/\nprojects.toml\n");
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
                "create store/ meta/ proposals/ cache/",
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
        // 缺省字段（无 user/gc/backup 段）落默认值
        let minimal = HubConfig::from_toml("schema_version = 1\n").unwrap();
        assert_eq!(minimal.user, UserIdentity::default());
        assert_eq!(minimal.gc.grace_days, 0);
        assert_eq!(minimal.backup.remind_days, 7);
    }

    #[test]
    fn save_registry_accepts_held_token() {
        let install = tempfile::tempdir().unwrap();
        let paths = HubPaths::new(install.path());
        // 凭证随取随用：持锁写登记簿成功，语句结束凭证即释放
        let reg = ProjectRegistry {
            last_backup: 42,
            ..Default::default()
        };
        save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        assert_eq!(load_registry(&paths).unwrap().last_backup, 42);
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
        reg.last_backup = 555;
        save_registry(&paths, &reg, &crate::lock::write_lock(&paths).unwrap()).unwrap();
        let back = load_registry(&paths).unwrap();
        assert_eq!(back.projects.len(), 1);
        assert_eq!(back.projects[0].last_applied, 200);
        assert_eq!(back.last_backup, 555);
    }
}
