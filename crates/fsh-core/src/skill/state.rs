//! 技能状态：两个占位（工作副本 / 归档副本）与各操作前置的单一住所。
//! 工作副本在 store 下、归档副本在 archive 下，两个面各自独立判定：
//! 一个键可能只在一侧、两侧都有、或两侧都没有。
//! 判据只看目录是否存在（`is_dir()`），不看同名普通文件；一次读盘取全两个事实。
//! 库管理与项目使用的全部生产点都来这里取占位与前置拒绝；改状态语义只动这一处。

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::ErrorCode;
use crate::paths::HubPaths;

/// 前置拒绝的结构化载荷：占位判定与各操作前置在这里统一构造，
/// 库管理与项目使用两套外层错误枚举各自 From 收编，
/// 对外文案与稳定码由外层变体持有
#[derive(Debug, Error)]
pub enum StateError {
    /// 两侧都没有副本
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// 只有归档副本，工作副本不在场
    #[error("skill 已归档: {key}")]
    Archived { key: String },
    /// 目标面已被同名键占用
    #[error("目标键已占用: {key}")]
    Conflict { key: String },
    /// 任一侧已有副本，而该操作要求两侧都空闲
    #[error("skill 已存在: {key}")]
    AlreadyExists { key: String },
}

impl ErrorCode for StateError {
    fn error_code(&self) -> &'static str {
        match self {
            StateError::SkillNotFound { .. } => "skill_not_found",
            StateError::Archived { .. } => "archived",
            StateError::Conflict { .. } => "conflict",
            StateError::AlreadyExists { .. } => "already_exists",
        }
    }
}

/// 一次读盘得到的两个占位：两个面各有没有副本，彼此独立
#[derive(Debug)]
pub struct SkillOccupancy {
    /// 查询的技能键
    key: String,
    /// hub 路径族：键在未占位那一面的落点由它派生
    paths: HubPaths,
    /// 工作副本（store 下）的目录，不在场为 None
    workdir: Option<PathBuf>,
    /// 归档副本（archive 下）的目录，不在场为 None
    archive: Option<PathBuf>,
}

/// 一次取全某键的两个占位：store 与 archive 两个目录各判一次，判据统一为目录存在。
/// 只看目录，不看仓库也不取写锁：预演与查询都能用
pub fn skill_occupancy(paths: &HubPaths, key: &str) -> SkillOccupancy {
    let workdir = paths.skill_workdir(key);
    let archive = paths.skill_archived(key);
    SkillOccupancy {
        key: key.to_string(),
        paths: paths.clone(),
        workdir: workdir.is_dir().then_some(workdir),
        archive: archive.is_dir().then_some(archive),
    }
}

impl SkillOccupancy {
    /// 工作副本占位：store 下没有工作副本时为 None
    pub fn workdir(&self) -> Option<&Path> {
        self.workdir.as_deref()
    }

    /// 归档副本占位：archive 下没有归档副本时为 None
    pub fn archive(&self) -> Option<&Path> {
        self.archive.as_deref()
    }

    /// 要求技能活跃：工作副本必须在场并作为该操作的来源。
    /// 两侧皆无报 skill_not_found，只有归档副本报 archived
    pub fn require_active(&self) -> Result<PathBuf, StateError> {
        match &self.workdir {
            Some(dir) => Ok(dir.clone()),
            None if self.archive.is_some() => Err(StateError::Archived {
                key: self.key.clone(),
            }),
            None => Err(StateError::SkillNotFound {
                key: self.key.clone(),
            }),
        }
    }

    /// 要求归档侧空闲并给出归档落点：已有归档副本报 conflict
    pub fn require_free_archive(&self) -> Result<PathBuf, StateError> {
        if self.archive.is_some() {
            return Err(StateError::Conflict {
                key: self.key.clone(),
            });
        }
        Ok(self.paths.skill_archived(&self.key))
    }

    /// 恢复的两端：归档副本必须在场、工作副本必须空闲，返回 (归档副本, 工作副本)。
    /// 工作副本已占 store 侧或两侧都有副本报 conflict，两侧皆无报 skill_not_found
    pub fn require_restorable(&self) -> Result<(PathBuf, PathBuf), StateError> {
        if self.workdir.is_some() {
            return Err(StateError::Conflict {
                key: self.key.clone(),
            });
        }
        match &self.archive {
            Some(from) => Ok((from.clone(), self.paths.skill_workdir(&self.key))),
            None => Err(StateError::SkillNotFound {
                key: self.key.clone(),
            }),
        }
    }

    /// 要求至少一侧副本在场：两侧皆无报 skill_not_found
    pub fn require_removable(&self) -> Result<(), StateError> {
        if self.workdir.is_none() && self.archive.is_none() {
            return Err(StateError::SkillNotFound {
                key: self.key.clone(),
            });
        }
        Ok(())
    }

    /// 要求两侧都空闲：任一侧已有副本报 already exists
    pub fn require_vacant(&self) -> Result<(), StateError> {
        if self.workdir.is_some() || self.archive.is_some() {
            return Err(StateError::AlreadyExists {
                key: self.key.clone(),
            });
        }
        Ok(())
    }
}
