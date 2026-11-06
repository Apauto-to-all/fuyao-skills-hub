//! 登记簿（projects.toml，机器写）：hub 侧的全部项目名单。
//! 用法的唯一记录——项目里不放 fsh 任何文件，引用查找只读这里。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::HubError;
use crate::paths::HubPaths;

// ---------- 项目登记簿（projects.toml，机器写） ----------

/// 登记簿：hub 侧全部项目名单。
/// 用法的唯一记录——项目里不放 fsh 任何文件，引用查找只读这里。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectRegistry {
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

impl ProjectRecord {
    /// 这条记录是不是这个目录：一切「是不是同一个项目」的判断都走这里
    pub fn matches(&self, project_dir: &Path) -> bool {
        crate::identity::path_identity(Path::new(&self.path))
            == crate::identity::path_identity(project_dir)
    }
}

impl ProjectRegistry {
    /// 登记或刷新一个项目：同一身份 upsert，时间戳取传入值
    pub fn upsert(&mut self, project_dir: &Path, now: u64) {
        let canon = project_dir
            .canonicalize()
            .unwrap_or_else(|_| project_dir.to_path_buf());
        // 存规范化原样：后续落点与存在性判断都拿它做文件操作，剥了前缀的长路径会失效
        let path = canon.display().to_string();
        match self.projects.iter_mut().find(|p| p.matches(&canon)) {
            Some(rec) => {
                // 命中即刷新：时间戳往前推，记录收敛成这次的规范化形态
                rec.last_used = now;
                rec.path = path;
            }
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
        Ok(text) => toml::from_str(&text).map_err(|e| {
            // 解析失败属登记簿不可读：在读出口记一次，上层不重复记
            tracing::error!(path = %paths.projects_toml().display(), cause = %e, "解析项目登记簿失败");
            HubError::from(e)
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectRegistry::default()),
        // 读失败在归属它的边界记一次，上层不重复记
        Err(e) => {
            tracing::error!(path = %paths.projects_toml().display(), cause = %e, "读取项目登记簿失败");
            Err(e.into())
        }
    }
}

/// 写回登记簿；凭证参数保证调用方已持全局写锁（先锁后写）
pub fn save_registry(
    paths: &HubPaths,
    registry: &ProjectRegistry,
    _token: &crate::lock::WriteToken,
) -> Result<(), HubError> {
    let target = paths.projects_toml();
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    // 写盘失败在归属它的边界记一次，上层不重复记
    fs::write(&target, toml::to_string(registry)?).map_err(|e| {
        tracing::error!(path = %target.display(), cause = %e, "写入项目登记簿失败");
        e
    })?;
    // 登记簿写入是状态迁移：留 INFO 以便重建「谁在何时登记了哪些项目」
    tracing::info!(path = %target.display(), count = registry.projects.len(), "项目登记簿写入完成");
    Ok(())
}
