//! 目标集合：一次 use / unuse 要服务的全部目标——可选的一个全局目标
//! 加零到多个项目目标。集合为空是一次硬错误，由 use / unuse 报 `no_target`。

use std::path::PathBuf;

use super::{ProjectError, canonical_project};
use crate::hub::Scope;
use crate::identity::{display_path, path_identity};

/// 一次调用的目标集合：可选的一个全局目标 + 零到多个项目目标
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    /// 是否服务全局作用域
    global: bool,
    /// 项目根目录（按调用方给的顺序，可重复）
    projects: Vec<PathBuf>,
}

impl Targets {
    /// 从命令行形状构造：`--global` 开关 + 可重复的 `--project`
    pub fn new(global: bool, projects: Vec<PathBuf>) -> Self {
        Self { global, projects }
    }

    /// 只服务全局这一个目标
    pub fn global() -> Self {
        Self::new(true, Vec::new())
    }

    /// 只服务一个项目目标
    pub fn project(project: impl Into<PathBuf>) -> Self {
        Self::new(false, vec![project.into()])
    }

    /// 是否要服务全局目标
    pub(crate) fn wants_global(&self) -> bool {
        self.global
    }

    /// 项目目标目录：按调用方给的顺序，同一目录可能重复出现
    pub(crate) fn project_dirs(&self) -> &[PathBuf] {
        &self.projects
    }
}

/// 一个目标的身份：全局作用域，或某个项目的绝对路径
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// 全局技能目录，一台机器一份
    Global,
    /// 项目根（规范化绝对路径）
    Project(PathBuf),
}

impl Anchor {
    /// 作用域：登记记录与 JSON 里的 scope
    pub(crate) fn scope(&self) -> Scope {
        match self {
            Anchor::Global => Scope::Global,
            Anchor::Project(_) => Scope::Project,
        }
    }

    /// 项目根显示路径；全局目标没有项目
    pub(crate) fn project(&self) -> Option<String> {
        match self {
            Anchor::Global => None,
            Anchor::Project(dir) => Some(display_path(dir)),
        }
    }

    /// 里程碑日志：项目目标记绝对路径，全局目标记作用域，字段顺序与日志规范一致
    pub(crate) fn log_milestone(&self, key: &str, count: usize, elapsed_ms: u64, message: &str) {
        match self {
            Anchor::Project(dir) => tracing::info!(
                key = %key,
                project = %display_path(dir),
                count = count,
                elapsed_ms = elapsed_ms,
                "{message}"
            ),
            Anchor::Global => tracing::info!(
                key = %key,
                scope = "global",
                count = count,
                elapsed_ms = elapsed_ms,
                "{message}"
            ),
        }
    }
}

/// 项目目标规范化与去重：项目根必须存在（否则 invalid_project），
/// 同一目录写两遍是同一个目标，只留第一次出现
pub(crate) fn dedupe_anchors(anchors: Vec<Anchor>) -> Result<Vec<Anchor>, ProjectError> {
    let mut seen: Vec<String> = Vec::new();
    let mut kept = Vec::new();
    for anchor in anchors {
        match anchor {
            Anchor::Global => kept.push(Anchor::Global),
            Anchor::Project(dir) => {
                let dir = canonical_project(&dir)?;
                let id = path_identity(&dir);
                if seen.contains(&id) {
                    tracing::debug!(project = %dir.display(), "重复项目目标跳过");
                    continue;
                }
                seen.push(id);
                kept.push(Anchor::Project(dir));
            }
        }
    }
    Ok(kept)
}

/// 目标集合展开成有序目标：全局目标在最前，其后按调用方给的顺序排列项目目标
pub(crate) fn anchors_of(targets: &Targets) -> Vec<Anchor> {
    let mut anchors: Vec<Anchor> = Vec::new();
    if targets.wants_global() {
        anchors.push(Anchor::Global);
    }
    anchors.extend(
        targets
            .project_dirs()
            .iter()
            .map(|dir| Anchor::Project(dir.clone())),
    );
    anchors
}
