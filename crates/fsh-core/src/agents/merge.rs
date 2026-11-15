//! 用户 TOML 覆盖合并：内置名单打底，`agents/<键>.toml` 整项覆盖或新增。
//! 内置表与用户 TOML 先落到同一份配置形状，再走同一条注册逻辑。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};
use std::time::Instant;

use serde::Deserialize;

use super::builtin::{BUILTIN, BuiltinAgent};
use super::{AgentError, AgentInfo, AgentSource, GlobalRoot, expand_global};
use crate::paths::HubPaths;

/// 一份 agent 配置的原样值：内置表与用户 TOML 都先落到这里。
/// 未知字段必须报错，不能静默忽略；两个技能位都可选，至少给一个
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentConfig {
    /// 可选显示名；缺省用键
    #[serde(default)]
    display_name: Option<String>,
    /// 可选：相对项目根的技能目录
    #[serde(default)]
    skills_dir: Option<String>,
    /// 可选：可移植写法的全局技能目录，展开后必须是绝对路径
    #[serde(default)]
    global_skills_dir: Option<String>,
}

impl AgentConfig {
    /// 内置表一行落到与 TOML 相同的形状：字段与可选性逐项对齐；
    /// 全局根适配声明只属于内置条目，随行带过来
    fn from_builtin(row: &BuiltinAgent) -> (Self, Option<GlobalRoot>) {
        (
            Self {
                display_name: Some(row.display_name.to_string()),
                skills_dir: row.skills_dir.map(str::to_string),
                global_skills_dir: row.global_skills_dir.map(str::to_string),
            },
            row.global_root,
        )
    }
}

/// 列出全部可用 agent（内置名单与用户 TOML 合并后的配置），按键排序。
/// 家目录在这里读一次并随注册传下去，全局目录同时在每个条目上展开成绝对路径
pub fn list(paths: &HubPaths) -> Result<Vec<AgentInfo>, AgentError> {
    let home = std::env::home_dir();
    let mut merged: BTreeMap<String, AgentInfo> = BTreeMap::new();
    for row in BUILTIN {
        let (cfg, root) = AgentConfig::from_builtin(row);
        let agent = register(
            row.key.to_string(),
            cfg,
            root,
            home.as_deref(),
            AgentSource::BuiltIn,
            &format!("内置表 {}", row.key),
        )?;
        merged.insert(agent.key.clone(), agent);
    }

    let dir = paths.agents();
    // 耗时起点：扫描是批量文件系统操作，完成时带 elapsed_ms
    let started = Instant::now();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        // 尚未初始化用户覆盖目录 = 只有内置项
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // 目录缺失也属扫描正常结束：count 为纯内置名单条目数
            tracing::info!(
                path = %dir.display(),
                count = merged.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "扫描 agent 目录完成"
            );
            return Ok(merged.into_values().collect());
        }
        // 扫描失败在归属它的边界记一次，上层不重复记
        Err(e) => {
            tracing::error!(path = %dir.display(), cause = %e, "扫描 agent 目录失败");
            return Err(e.into());
        }
    };
    for entry in entries {
        let file = entry?.path();
        if !file.is_file() || file.extension().and_then(|e| e.to_str()) != Some("toml") {
            // 跳过非 TOML 条目属可恢复的过滤决策：留 WARN 便于发现放错位置的文件
            tracing::warn!(path = %file.display(), "agent 配置条目不是 TOML，跳过");
            continue;
        }
        let at = file.display().to_string();
        let key = file
            .file_stem()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| invalid(&at, "文件名不是合法的 UTF-8 agent 键".into()))?
            .to_string();
        let text = fs::read_to_string(&file)?;
        let cfg: AgentConfig =
            toml::from_str(&text).map_err(|e| invalid(&at, format!("TOML 解析失败: {e}")))?;
        let agent = register(key, cfg, None, home.as_deref(), AgentSource::User, &at)?;
        merged.insert(agent.key.clone(), agent);
    }
    // 扫描完成是外部交互里程碑：count 为内置名单与用户覆盖合并后的 agent 总数
    tracing::info!(
        path = %dir.display(),
        count = merged.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "扫描 agent 目录完成"
    );
    Ok(merged.into_values().collect())
}

/// 注册一条 agent：内置表与用户 TOML 共用这一套校验与转换，规则只有一份。
/// `root` 是内置条目的全局根适配声明，用户配置一律 None；`home` 是加载时读到的家目录；
/// `at` 是报错时点名的位置（配置文件路径或内置表条目）
fn register(
    key: String,
    cfg: AgentConfig,
    root: Option<GlobalRoot>,
    home: Option<&Path>,
    source: AgentSource,
    at: &str,
) -> Result<AgentInfo, AgentError> {
    let skills_dir = match cfg.skills_dir {
        Some(dir) => {
            validate_skills_dir(&dir).map_err(|reason| invalid(at, reason))?;
            Some(dir)
        }
        None => None,
    };
    // 展开即校验：非法写法在读名单时报错，展开结果直接留作全局落点；
    // 内置声明的回落是静默的，校验同样按声明解析
    let global_dir = match cfg.global_skills_dir.as_deref() {
        Some(raw) => Some(expand_global(raw, root, home, at)?),
        None => None,
    };
    if skills_dir.is_none() && cfg.global_skills_dir.is_none() {
        return Err(invalid(
            at,
            "skills_dir 与 global_skills_dir 至少要给一个".into(),
        ));
    }
    let display_name = cfg
        .display_name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| key.clone());
    Ok(AgentInfo {
        key,
        display_name,
        skills_dir,
        global_skills_dir: cfg.global_skills_dir,
        global_dir,
        source,
    })
}

/// `skills_dir` 必须是项目内的相对目录：拒绝空值、绝对路径、盘符与任何
/// 非普通路径段（`.`、`..`、根起点），落点因此不会越出项目根
fn validate_skills_dir(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("skills_dir 不能为空".into());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(format!("skills_dir 必须是相对路径: {raw}"));
    }
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!(
            "skills_dir 不得包含 `.`、`..` 或盘符等非普通路径段: {raw}"
        ));
    }
    Ok(())
}

/// 配置错误的统一构造（带上报错位置）
fn invalid(at: &str, reason: String) -> AgentError {
    AgentError::InvalidConfig {
        path: at.to_string(),
        reason,
    }
}
