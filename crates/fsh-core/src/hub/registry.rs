//! 登记簿（projects.toml，机器写）：hub 侧的全部用法记录。
//! 用法的唯一记录——项目里不放 fsh 任何文件，引用查找只读这里。
//! 三层形状：作用域（项目 / 全局）→ 身份（项目路径；全局只有一层）→ 记录。
//! 项目身份写成表键，于是每条技能记录的 header 自带归属，不必向上翻页判定。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::HubError;
use crate::identity::display_path;
use crate::paths::HubPaths;

// ---------- 登记簿（projects.toml，机器写） ----------

/// 用法的作用域：项目级或全局级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// 铺给某个项目（默认作用域）
    #[default]
    Project,
    /// 铺给全局技能目录
    Global,
}

/// 登记簿：hub 侧全部用法记录，按作用域分两段。
/// 用法的唯一记录——项目里不放 fsh 任何文件，引用查找只读这里。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectRegistry {
    /// 项目用法：规范化绝对路径 → 该项目记录；没有记录时不写进文件
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub projects: BTreeMap<String, ProjectRecord>,
    /// 全局用法：只有一个作用域，没有记录时整段不写进文件
    #[serde(default, skip_serializing_if = "GlobalSkills::is_empty")]
    pub globals: GlobalSkills,
}

/// 全局作用域下的记录：没有身份层，技能直接挂在下面
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GlobalSkills {
    /// 全局使用的技能
    #[serde(default)]
    pub skills: Vec<ProjectSkill>,
}

impl GlobalSkills {
    /// 空记录：写盘时整段略去，文件里不留空表
    fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// 单条项目记录：项目身份是表键，这里只留该项目的状态
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProjectRecord {
    /// 最后一次被 fsh 操作的时间（Unix 秒）
    pub last_used: u64,
    /// 该项目使用的技能
    #[serde(default)]
    pub skills: Vec<ProjectSkill>,
}

/// 单条技能登记：给哪些 agent 用、实际落成哪种链接
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectSkill {
    /// hub 里的唯一键（键末段就是 name，不另存）
    pub key: String,
    /// 给哪些 agent 用（通用位取值 universal）
    pub agents: Vec<String>,
    /// 实际采用的链接方式（一条记录一个值，作用于 agents 列出的全部 agent）
    pub link: crate::link::LinkKind,
}

/// 一条使用记录：作用域、项目、链接方式与 agent 名单。
/// 每条登记记录各占一行；agent 名单与链接方式按登记原样带出，不跨记录合并、不补默认值。
/// 作用域是全局时项目必为空——`usages` 是唯一构造点，按作用域决定写不写项目
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillUsage {
    /// 这条记录属于哪个作用域
    pub scope: Scope,
    /// 项目根路径（展示形态，界面直接显示）；全局记录没有项目，不写
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// 这条记录登记时实际采用的链接方式
    pub link: crate::link::LinkKind,
    /// 这条记录登记的 agent 键
    pub agents: Vec<String>,
}

/// 项目身份键：规范化绝对路径。
/// 同一目录的不同写法经规范化后收敛成同一个键，查表即按键命中；
/// 取规范化原样而不剥 Windows 扩展前缀——剥了前缀的超长路径做不了文件操作。
pub(crate) fn project_key(project_dir: &Path) -> String {
    // 目录已不存在时按原写法取键：死记录也要能凭原写法读回来
    project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf())
        .display()
        .to_string()
}

impl ProjectRegistry {
    /// 登记或刷新一个项目：身份命中即刷新时间戳
    pub fn upsert(&mut self, project_dir: &Path, now: u64) {
        self.projects
            .entry(project_key(project_dir))
            .or_default()
            .last_used = now;
    }

    /// 按目录取项目记录：与 upsert 共用同一个身份键
    pub fn project(&self, project_dir: &Path) -> Option<&ProjectRecord> {
        self.projects.get(&project_key(project_dir))
    }

    /// 按目录取可改的项目记录
    pub fn project_mut(&mut self, project_dir: &Path) -> Option<&mut ProjectRecord> {
        self.projects.get_mut(&project_key(project_dir))
    }

    /// 某技能的全部使用记录：全局段在前，项目段按其路径序在后。
    /// 每条登记记录占一行——agent 名单与链接方式原样带出，跨记录不做合并
    pub fn usages(&self, key: &str) -> Vec<SkillUsage> {
        let mut usages: Vec<SkillUsage> = self
            .globals
            .skills
            .iter()
            .filter(|skill| skill.key == key)
            .map(|skill| SkillUsage {
                scope: Scope::Global,
                project: None,
                link: skill.link,
                agents: skill.agents.clone(),
            })
            .collect();
        usages.extend(self.projects.iter().flat_map(|(path, record)| {
            // 展示形态：去掉 Windows 扩展前缀；传回时按身份键规范化仍能命中
            let project = display_path(Path::new(path));
            record
                .skills
                .iter()
                .filter(|skill| skill.key == key)
                .map(move |skill| SkillUsage {
                    scope: Scope::Project,
                    project: Some(project.clone()),
                    link: skill.link,
                    agents: skill.agents.clone(),
                })
        }));
        usages
    }

    /// 全局段的只读访问：全局只有一个作用域，没有身份层
    pub fn global(&self) -> &GlobalSkills {
        &self.globals
    }

    /// 全局段的可改访问：写入全局用法走这里
    pub fn global_mut(&mut self) -> &mut GlobalSkills {
        &mut self.globals
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
    // 先写临时文件再改名：读侧（gc 预演、doctor）不持锁，截断写在并发读时会露出半截 toml
    let staging = target.with_extension("toml.tmp");
    let text = toml::to_string(registry)?;
    fs::write(&staging, text).map_err(|e| {
        tracing::error!(path = %staging.display(), cause = %e, "写入项目登记簿失败");
        e
    })?;
    fs::rename(&staging, &target).map_err(|e| {
        tracing::error!(path = %target.display(), cause = %e, "写入项目登记簿失败");
        e
    })?;
    // 登记簿写入是状态迁移：留 INFO 以便重建「谁在何时登记了哪些用法」
    tracing::info!(
        path = %target.display(),
        count = registry.projects.len() + registry.globals.skills.len(),
        "用法登记簿写入完成"
    );
    Ok(())
}
