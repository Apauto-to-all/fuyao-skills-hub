//! 登记簿（projects.toml，机器写）：hub 侧的全部用法记录。
//! 用法的唯一记录——项目里不放 fsh 任何文件，引用查找只读这里。
//! 三层形状：作用域（项目 / 全局）→ 身份（项目路径；全局只有一层）→ 记录。
//! 项目身份写成表键，于是每条技能记录的 header 自带归属，不必向上翻页判定。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::HubError;
use crate::identity::{display_path, project_key};
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
    projects: BTreeMap<String, ProjectRecord>,
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

/// 行走中的一位：一条登记记录及其归属。
/// 项目记录带出的路径就是登记键原样（规范化绝对路径，含 Windows 扩展前缀），
/// 消费方拿去做文件操作不必再懂「键是路径」
#[derive(Debug, Clone, PartialEq)]
pub struct RecordRef<'a> {
    scope: Scope,
    project: Option<&'a Path>,
    record: &'a ProjectSkill,
}

impl<'a> RecordRef<'a> {
    /// 这条记录属于哪个作用域
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// 项目根（登记键原样）；全局记录没有项目
    pub fn project_dir(&self) -> Option<&'a Path> {
        self.project
    }

    /// 这条登记记录
    pub fn skill(&self) -> &'a ProjectSkill {
        self.record
    }
}

impl ProjectRegistry {
    /// 登记或刷新一个项目身份：身份命中即刷新时间戳，不带技能记录。
    /// 逐技能登记走 record_use；这条出口服务只碰身份不挂记录的场景
    pub(crate) fn upsert(&mut self, project_dir: &Path, now: u64) {
        self.projects
            .entry(project_key(project_dir))
            .or_default()
            .last_used = now;
    }

    /// 按目录取项目记录：与登记共用同一个身份键
    pub fn project(&self, project_dir: &Path) -> Option<&ProjectRecord> {
        self.projects.get(&project_key(project_dir))
    }

    /// 逐记录访问全部用法：全局段在前，项目段按登记键序在后。
    /// 每条登记记录各占一位；死项目记录照常行走，身份按登记键原样带出。
    /// 两段行走只住在这里，消费方从这里取记录
    pub fn records(&self) -> impl Iterator<Item = RecordRef<'_>> {
        self.globals
            .skills
            .iter()
            .map(|skill| RecordRef {
                scope: Scope::Global,
                project: None,
                record: skill,
            })
            .chain(self.projects.iter().flat_map(|(key, record)| {
                let dir = Path::new(key);
                record.skills.iter().map(move |skill| RecordRef {
                    scope: Scope::Project,
                    project: Some(dir),
                    record: skill,
                })
            }))
    }

    /// 某技能的全部使用记录：全局段在前，项目段按其路径序在后。
    /// 每条登记记录占一行——agent 名单与链接方式原样带出，跨记录不做合并
    pub fn usages(&self, key: &str) -> Vec<SkillUsage> {
        self.records()
            .filter(|record| record.skill().key == key)
            .map(|record| SkillUsage {
                scope: record.scope(),
                // 展示形态：去掉 Windows 扩展前缀；传回时按身份键规范化仍能命中
                project: record.project_dir().map(display_path),
                link: record.skill().link,
                agents: record.skill().agents.clone(),
            })
            .collect()
    }

    /// 项目段的身份清单：只有项目记录的目录，按登记键序
    pub fn project_dirs(&self) -> impl Iterator<Item = &Path> {
        self.projects.keys().map(Path::new)
    }

    /// 撤一条技能用法：`remaining` 为 None 时整条记录删除（名单点满），
    /// 否则把记录的 agent 名单换成剩余名单。项目没有别的技能时连项目记录一起删
    pub(crate) fn remove_use(
        &mut self,
        anchor: &crate::project::Anchor,
        key: &str,
        remaining: Option<&[String]>,
    ) {
        let skills: &mut Vec<ProjectSkill> = match anchor {
            crate::project::Anchor::Project(dir) => {
                match self.projects.get_mut(&project_key(dir)) {
                    Some(record) => &mut record.skills,
                    None => return,
                }
            }
            crate::project::Anchor::Global => &mut self.globals.skills,
        };
        match remaining {
            None => skills.retain(|s| s.key != key),
            Some(rest) => {
                for skill in skills.iter_mut().filter(|s| s.key == key) {
                    skill.agents = rest.to_vec();
                }
            }
        }
        self.prune_empty_projects();
    }

    /// 全部记录里的旧键引用改写为新键：agents / link / last_used 原样保留
    pub(crate) fn rewrite_key(&mut self, from: &str, to: &str) {
        for record in self.projects.values_mut() {
            for skill in &mut record.skills {
                if skill.key == from {
                    skill.key = to.to_string();
                }
            }
        }
        for skill in &mut self.globals.skills {
            if skill.key == from {
                skill.key = to.to_string();
            }
        }
    }

    /// 登记一条技能用法：项目目标先登记身份再挂技能记录，全局目标直接挂全局段。
    /// 两段的登记顺序规则只住在这里，调用方交记录本身即可
    pub(crate) fn record_use(
        &mut self,
        anchor: &crate::project::Anchor,
        record: ProjectSkill,
        now: u64,
    ) {
        match anchor {
            crate::project::Anchor::Project(dir) => {
                self.upsert(dir, now);
                self.projects
                    .get_mut(&project_key(dir))
                    .expect("登记身份后必能取到记录")
                    .skills
                    .push(record);
            }
            crate::project::Anchor::Global => self.globals.skills.push(record),
        }
    }

    /// 全局段的只读访问：全局只有一个作用域，没有身份层
    pub fn global(&self) -> &GlobalSkills {
        &self.globals
    }

    /// 删掉已没有技能记录的项目记录：项目记录只随撤回清扫，
    /// 项目段里不留没有技能的项目；全局段由 is_empty 决定是否写进文件
    pub fn prune_empty_projects(&mut self) {
        self.projects.retain(|_, record| !record.skills.is_empty());
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
