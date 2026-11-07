//! 用户 TOML 覆盖合并：内置名单打底，`agents/<键>.toml` 整项覆盖或新增。
//! 只做配置解析与校验，不碰登记簿。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path};
use std::time::Instant;

use serde::Deserialize;

use super::builtin::BUILTIN;
use super::portable_dir;
use super::{AgentError, AgentInfo, AgentSource};
use crate::paths::{HubPaths, PathsError};

/// 用户 TOML 的形状；未知字段必须报错，不能静默忽略。
/// 两个目录至少给一个：只给全局目录的 agent 也能存在
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserAgentConfig {
    /// 可选显示名；缺省用文件名
    #[serde(default)]
    display_name: Option<String>,
    /// 可选：相对项目根的技能目录
    #[serde(default)]
    skills_dir: Option<String>,
    /// 可选：可移植写法的全局技能目录，展开后必须是绝对路径
    #[serde(default)]
    global_skills_dir: Option<String>,
}

/// 合并内置名单与用户 TOML，只做配置解析，不碰登记簿
pub(crate) fn load_agents(paths: &HubPaths) -> Result<Vec<AgentInfo>, AgentError> {
    // 内置表用 `~` 写全局目录，读名单一律先要家目录
    let home = std::env::home_dir().ok_or(PathsError::NoHome)?;
    let lookup = |name: &str| std::env::var(name).ok();
    let mut merged: BTreeMap<String, AgentInfo> = BUILTIN
        .iter()
        .map(|(key, display_name, skills_dir, global_skills_dir)| {
            Ok((
                (*key).to_string(),
                AgentInfo {
                    key: (*key).to_string(),
                    display_name: (*display_name).to_string(),
                    skills_dir: Some((*skills_dir).to_string()),
                    global_skills_dir: expand_builtin_global(
                        key,
                        *global_skills_dir,
                        &home,
                        &lookup,
                    )?,
                    source: AgentSource::BuiltIn,
                    usages: Vec::new(),
                },
            ))
        })
        .collect::<Result<_, AgentError>>()?;

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
        let key = file
            .file_stem()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| invalid(&file, "文件名不是合法的 UTF-8 agent 键".into()))?
            .to_string();
        let text = fs::read_to_string(&file)?;
        let cfg: UserAgentConfig =
            toml::from_str(&text).map_err(|e| invalid(&file, format!("TOML 解析失败: {e}")))?;
        let skills_dir = match cfg.skills_dir {
            Some(dir) => {
                validate_skills_dir(&dir).map_err(|reason| invalid(&file, reason))?;
                Some(dir)
            }
            None => None,
        };
        let global_skills_dir = match cfg.global_skills_dir {
            Some(raw) => Some(
                portable_dir::expand(&raw, &home, &lookup)
                    .map_err(|reason| invalid(&file, format!("global_skills_dir {reason}")))?
                    .display()
                    .to_string(),
            ),
            None => None,
        };
        if skills_dir.is_none() && global_skills_dir.is_none() {
            return Err(invalid(
                &file,
                "skills_dir 与 global_skills_dir 至少要给一个".into(),
            ));
        }
        let display_name = cfg
            .display_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| key.clone());
        merged.insert(
            key.clone(),
            AgentInfo {
                key,
                display_name,
                skills_dir,
                global_skills_dir,
                source: AgentSource::User,
                usages: Vec::new(),
            },
        );
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

/// 内置表一行的全局目录：没写全局位就是 None，写了按可移植写法展开。
/// 展开失败即内置表写错，报错点名是哪一条
fn expand_builtin_global(
    key: &str,
    raw: Option<&str>,
    home: &Path,
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<String>, AgentError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let expanded = portable_dir::expand(raw, home, lookup)
        .map_err(|reason| invalid(Path::new(&format!("内置表 {key}")), reason))?;
    Ok(Some(expanded.display().to_string()))
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

/// 用户配置错误的统一构造（带文件路径）
fn invalid(path: &Path, reason: String) -> AgentError {
    AgentError::InvalidConfig {
        path: path.display().to_string(),
        reason,
    }
}
