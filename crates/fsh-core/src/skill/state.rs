//! 技能状态：唯一键三态判定与版本清单的单一住所。
//! 一次查询回答「这个键现在什么状态、有哪些版本、最新是哪版」；
//! 判定以工作副本为主——有工作副本即活跃（版本清单可为空），
//! 无副本有版本 tag 即归档，两者皆无即不存在。
//! 库管理与项目使用的全部生产点都来这里判定三态与取版本；
//! 改归档语义只动这一处。

use thiserror::Error;

use crate::git::{self, TagEntry};
use crate::paths::HubPaths;

/// 唯一键的三态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillState {
    /// 工作副本在场（版本清单可为空）
    Active,
    /// 无工作副本、有版本 tag
    Archived,
    /// 既无工作副本也无版本 tag
    Missing,
}

/// 三态错误的结构化载荷：状态模块判定并统一构造，
/// 库管理与项目使用两套外层错误枚举各自 From 收编，
/// 对外文案与稳定码由外层变体持有，两边长期不同的说法原样保留
#[derive(Debug, Error)]
pub enum StateError {
    /// 既无工作副本也无版本 tag
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// 无工作副本、有版本 tag
    #[error("skill 已归档: {key}")]
    Archived { key: String },
    /// 指定版本不在版本清单里
    #[error("skill 没有该版本: {key}@{version}")]
    VersionNotFound { key: String, version: String },
}

/// 一次查询的全量状态：三态、版本条目（新在前）与由它派生的最新版本
#[derive(Debug, Clone)]
pub struct SkillStatus {
    /// 查询的技能键
    key: String,
    /// 三态
    state: SkillState,
    /// 版本条目（新在前；活跃形态可为空）
    entries: Vec<TagEntry>,
}

/// 一次取全某键的状态：读版本 tag 与工作副本事实并归类三态。
/// 只读姿势：不取写锁，预演与查询都能用
pub fn skill_status(
    repo: &git::Repository,
    paths: &HubPaths,
    key: &str,
) -> Result<SkillStatus, git::GitError> {
    let entries = git::tag_entries(repo, key)?;
    let state = if paths.skill_workdir(key).is_dir() {
        SkillState::Active
    } else if entries.is_empty() {
        SkillState::Missing
    } else {
        SkillState::Archived
    };
    Ok(SkillStatus {
        key: key.to_string(),
        state,
        entries,
    })
}

impl SkillStatus {
    /// 查询的技能键
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 三态
    pub fn state(&self) -> SkillState {
        self.state
    }

    /// 键是否在场：活跃与归档都算，只有不存在的键返回 false
    pub fn is_present(&self) -> bool {
        self.state != SkillState::Missing
    }

    /// 是否有任何版本 tag
    pub fn has_versions(&self) -> bool {
        !self.entries.is_empty()
    }

    /// 版本清单（新在前）
    pub fn versions(&self) -> impl Iterator<Item = &str> + '_ {
        self.entries.iter().map(|e| e.version.as_str())
    }

    /// 指定版本是否在版本清单里
    pub fn has_version(&self, version: &str) -> bool {
        self.versions().any(|have| have == version)
    }

    /// 最新版本（版本清单表头；无任何版本 tag 时为 None）
    pub fn latest(&self) -> Option<&str> {
        self.entries.first().map(|e| e.version.as_str())
    }

    /// 版本条目（新在前，含发布提交与时间）：版本时间线由此展开
    pub fn entries(&self) -> &[TagEntry] {
        &self.entries
    }

    /// 要求键在场：不存在的键给出 skill_not_found
    pub fn require_present(&self) -> Result<(), StateError> {
        match self.state {
            SkillState::Active | SkillState::Archived => Ok(()),
            SkillState::Missing => Err(StateError::SkillNotFound {
                key: self.key.clone(),
            }),
        }
    }

    /// 要求键活跃：不存在的键给出 skill_not_found，归档键给出 archived
    pub fn require_active(&self) -> Result<(), StateError> {
        match self.state {
            SkillState::Active => Ok(()),
            SkillState::Archived => Err(StateError::Archived {
                key: self.key.clone(),
            }),
            SkillState::Missing => Err(StateError::SkillNotFound {
                key: self.key.clone(),
            }),
        }
    }

    /// 要求最新版本在场：没有版本 tag 的键没有可依据的版本链，
    /// 按不存在拒绝
    pub fn require_latest(&self) -> Result<&str, StateError> {
        self.latest().ok_or_else(|| StateError::SkillNotFound {
            key: self.key.clone(),
        })
    }

    /// 要求版本在清单里：不在给出 version_not_found
    pub fn require_version(&self, version: &str) -> Result<(), StateError> {
        if self.has_version(version) {
            Ok(())
        } else {
            Err(StateError::VersionNotFound {
                key: self.key.clone(),
                version: version.to_string(),
            })
        }
    }
}
