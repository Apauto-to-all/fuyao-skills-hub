//! 登记簿（projects.toml，机器写）：hub 侧的全部项目名单与备份时间戳。
//! 用法的唯一记录——项目里不放 fsh 任何文件，引用查找只读这里。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::HubError;
use crate::paths::HubPaths;

// ---------- 项目登记簿（projects.toml，机器写） ----------

/// 登记簿：hub 侧全部项目名单 + 备份时间戳。
/// 用法的唯一记录——项目里不放 fsh 任何文件，引用查找只读这里。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectRegistry {
    /// 最后一次成功备份时间（Unix 秒，0 = 从未备份）
    #[serde(default)]
    pub last_backup: u64,
    /// 全部已登记项目
    #[serde(default)]
    pub projects: Vec<ProjectRecord>,
}

/// 单条项目登记：一个项目 + 该项目下的若干技能
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectRecord {
    /// 项目根绝对路径
    pub path: String,
    /// 最后一次被 fsh 操作的时间（Unix 秒）
    pub last_used: u64,
    /// 该项目使用的技能
    #[serde(default)]
    pub skills: Vec<ProjectSkill>,
}

/// 单条技能登记：给哪些 agent 用、用哪个版本、实际落成哪种链接
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectSkill {
    /// hub 里的唯一键（键末段就是 name，不另存）
    pub key: String,
    /// 固定版本；缺省 = 跟最新
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 给哪些 agent 用（通用位取值 universal）
    pub agents: Vec<String>,
    /// 实际采用的链接方式（一条记录一个值，作用于 agents 列出的全部 agent）
    pub link: crate::link::LinkKind,
}

impl ProjectRegistry {
    /// 登记或刷新一个项目（同路径 upsert，时间戳取传入值）
    pub fn upsert(&mut self, project_dir: &Path, now: u64) {
        let canon = project_dir
            .canonicalize()
            .unwrap_or_else(|_| project_dir.to_path_buf());
        let path = canon.display().to_string();
        match self.projects.iter_mut().find(|p| p.path == path) {
            Some(rec) => rec.last_used = now,
            None => self.projects.push(ProjectRecord {
                path,
                last_used: now,
                skills: Vec::new(),
            }),
        }
    }

    /// 引用某技能的项目路径清单（任一项目记录里出现该键即为引用）
    pub fn referencing_projects(&self, key: &str) -> Vec<String> {
        self.projects
            .iter()
            .filter(|p| p.skills.iter().any(|s| s.key == key))
            .map(|p| p.path.clone())
            .collect()
    }
}

/// 读取登记簿；文件缺失视为空登记簿（尚未用过任何项目）
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
