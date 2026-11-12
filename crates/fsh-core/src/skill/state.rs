//! 技能状态：两个占位（工作副本 / 归档副本）与各操作前置的单一住所。
//! 工作副本在 store 下、归档副本在 archive 下，两个面各自独立判定：
//! 一个键可能只在一侧、两侧都有、或两侧都没有。
//! 判据只看目录是否存在（`is_dir()`），不看同名普通文件；一次读盘取全两个事实。
//! 库管理与项目使用的全部生产点都来这里取占位与前置拒绝；改状态语义只动这一处。

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::paths::HubPaths;

/// 前置拒绝的结构化载荷：占位判定与各操作前置在这里统一构造，
/// 库管理侧的 [`crate::skill::SkillError`] 逐变体 From 收编，
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

/// 要求工作副本在场这条前置的拒绝：只有「不存在」与「已归档」两种结果，
/// 目标面占用不属于这条判定，收窄后调用方不必处理走不到的分支
#[derive(Debug, Error)]
pub enum ActiveError {
    /// 两侧都没有副本
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// 只有归档副本，工作副本不在场
    #[error("skill 已归档: {key}")]
    Archived { key: String },
}

/// 恢复的两端：来源是归档副本，落点是 store 工作副本。
/// 两个字段同名类型，用具名结构区分，换序不会静默成立
#[derive(Debug)]
pub struct RestoreEnds {
    /// 归档副本目录：移动的来源
    pub from: PathBuf,
    /// store 工作副本目录：移动的落点
    pub to: PathBuf,
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
    /// 两侧皆无报不存在，只有归档副本报已归档
    pub fn require_active(&self) -> Result<PathBuf, ActiveError> {
        match &self.workdir {
            Some(dir) => Ok(dir.clone()),
            None if self.archive.is_some() => Err(ActiveError::Archived {
                key: self.key.clone(),
            }),
            None => Err(ActiveError::SkillNotFound {
                key: self.key.clone(),
            }),
        }
    }

    /// 要求归档侧空闲并给出归档落点：已有归档副本报冲突。
    /// 判据只认目录——归档侧的同名普通文件不构成占位，落点仍判为空闲，
    /// 移动那一步才会在文件系统上失败
    pub fn require_free_archive(&self) -> Result<PathBuf, StateError> {
        if self.archive.is_some() {
            return Err(StateError::Conflict {
                key: self.key.clone(),
            });
        }
        Ok(self.paths.skill_archived(&self.key))
    }

    /// 恢复的两端：归档副本必须在场、工作副本必须空闲。
    /// 工作副本已占 store 侧或两侧都有副本报冲突，两侧皆无报不存在
    pub fn require_restorable(&self) -> Result<RestoreEnds, StateError> {
        if self.workdir.is_some() {
            return Err(StateError::Conflict {
                key: self.key.clone(),
            });
        }
        match &self.archive {
            Some(from) => Ok(RestoreEnds {
                from: from.clone(),
                to: self.paths.skill_workdir(&self.key),
            }),
            None => Err(StateError::SkillNotFound {
                key: self.key.clone(),
            }),
        }
    }

    /// 要求至少一侧副本在场：两侧皆无报不存在
    pub fn require_removable(&self) -> Result<(), StateError> {
        if self.workdir.is_none() && self.archive.is_none() {
            return Err(StateError::SkillNotFound {
                key: self.key.clone(),
            });
        }
        Ok(())
    }

    /// 要求两侧都空闲：任一侧已有副本报已存在
    pub fn require_vacant(&self) -> Result<(), StateError> {
        if self.workdir.is_some() || self.archive.is_some() {
            return Err(StateError::AlreadyExists {
                key: self.key.clone(),
            });
        }
        Ok(())
    }
}
