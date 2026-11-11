//! 技能状态：唯一键三态判定与归档目录的单一住所。
//! 工作副本在 store 下即活跃，归档副本在 archive 下即归档，
//! 两处皆无即不存在；判定只看两个目录的存在性。
//! 库管理与项目使用的全部生产点都来这里判定状态；改状态语义只动这一处。

use thiserror::Error;

use crate::paths::HubPaths;

/// 唯一键的三态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillState {
    /// 工作副本在 store 下
    Active,
    /// 归档副本在 archive 下（store 下无工作副本）
    Archived,
    /// store 与 archive 下皆无
    Missing,
}

/// 三态错误的结构化载荷：状态模块判定并统一构造，
/// 库管理与项目使用两套外层错误枚举各自 From 收编，
/// 对外文案与稳定码由外层变体持有
#[derive(Debug, Error)]
pub enum StateError {
    /// store 与 archive 下皆无
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// 归档态（store 下无工作副本）
    #[error("skill 已归档: {key}")]
    Archived { key: String },
}

/// 一次查询的全量状态
#[derive(Debug, Clone)]
pub struct SkillStatus {
    /// 查询的技能键
    key: String,
    /// 三态
    state: SkillState,
}

/// 一次取全某键的状态：读 store 与 archive 两个目录的存在性并归类。
/// 判据只看目录，不看仓库也不取写锁：预演与查询都能用
pub fn skill_status(paths: &HubPaths, key: &str) -> SkillStatus {
    let state = if paths.skill_workdir(key).is_dir() {
        SkillState::Active
    } else if paths.skill_archived(key).is_dir() {
        SkillState::Archived
    } else {
        SkillState::Missing
    };
    SkillStatus {
        key: key.to_string(),
        state,
    }
}

impl SkillStatus {
    /// 三态
    pub fn state(&self) -> SkillState {
        self.state
    }

    /// 键是否在场：活跃与归档都算，只有不存在的键返回 false
    pub fn is_present(&self) -> bool {
        self.state != SkillState::Missing
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
}
