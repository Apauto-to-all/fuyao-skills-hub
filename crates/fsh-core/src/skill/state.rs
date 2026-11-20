//! 技能状态：两个占位（工作副本 / 归档副本）、落点占用与技能识别的单一住所。
//! 工作副本在 store 下、归档副本在 archive 下，两个面各自独立判定：
//! 一个键可能只在一侧、两侧都有、或两侧都没有。
//! 两套判据各管一件事：**占位**只看目录是否存在（`is_dir()`），回答「副本在哪」；
//! **落点闸**看路径上有没有任何东西，回答「能不能往这放」——由 [`occupancy`] 一处给出。
//! 技能识别由 [`walk_face`] 一处给出：命中 `SKILL.md` 的目录是技能根，子树归技能自身。
//! 库管理与项目使用的全部生产点都来这里取占位、落点占用与识别结果；改状态语义只动这一处。

use std::fmt;
use std::fs;
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

/// 路径上有什么：占位看副本在哪，落点闸看能不能往这放
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Occupancy {
    /// 路径上什么都没有
    Nothing,
    /// 普通目录
    Dir,
    /// 普通文件
    File,
    /// 符号链接或目录联接（在盘上都表现为 reparse point）
    Link,
}

impl fmt::Display for Occupancy {
    /// 人读文案：落点报错靠它说清占着的是什么
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Occupancy::Nothing => "不存在",
            Occupancy::Dir => "普通目录",
            Occupancy::File => "普通文件",
            Occupancy::Link => "符号链接",
        })
    }
}

/// 判一次路径占用：链接优先于目录——目录联接在盘上也算链接。
/// 用 symlink_metadata 而不是 metadata：断链也要算占着，不能当空位
pub(crate) fn occupancy(path: &Path) -> Occupancy {
    let Ok(meta) = path.symlink_metadata() else {
        return Occupancy::Nothing;
    };
    if meta.file_type().is_symlink() {
        Occupancy::Link
    } else if meta.is_dir() {
        Occupancy::Dir
    } else {
        Occupancy::File
    }
}

/// 一个面里的全部技能键（只取技能根），按遍历顺序产出。
/// 选择器的候选面由这一处给：识别只看 `SKILL.md`
pub(crate) fn face_keys(face_dir: &Path) -> impl Iterator<Item = String> {
    walk_face(face_dir)
        .filter(|skill| skill.has_skill_md)
        .map(|skill| skill.key)
}

/// 一个面里走到的目录：键、目录、`SKILL.md` 是否在。
/// `has_skill_md` 为真的目录是技能根；为假的是技能根的前缀层或残留目录
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FoundSkill {
    /// 相对面根的正斜杠键
    pub key: String,
    /// 目录绝对路径
    pub dir: PathBuf,
    /// 该目录是否是技能根
    pub has_skill_md: bool,
}

/// 遍历一个面下的目录树，按 `SKILL.md` 判定技能根。
/// 命中 `SKILL.md` 的目录是技能根，其子树不再产出条目：子树属于技能自身，
/// 里面的目录既不是技能，也不是键空间
pub(crate) fn walk_face(face_dir: &Path) -> impl Iterator<Item = FoundSkill> {
    WalkFace {
        pending: vec![(face_dir.to_path_buf(), String::new())],
    }
}

/// 深度优先的目录栈：一次弹出一个目录，非技能根才把子目录压栈
struct WalkFace {
    /// 待走目录与它的键（面根本身是空键，不产出条目）
    pending: Vec<(PathBuf, String)>,
}

impl Iterator for WalkFace {
    type Item = FoundSkill;

    fn next(&mut self) -> Option<FoundSkill> {
        while let Some((dir, key)) = self.pending.pop() {
            // 面根不是任何技能：只展开，不产出
            let has_skill_md = !key.is_empty() && dir.join("SKILL.md").is_file();
            if !has_skill_md {
                push_children(&dir, &key, &mut self.pending);
            }
            if !key.is_empty() {
                return Some(FoundSkill {
                    key,
                    dir,
                    has_skill_md,
                });
            }
        }
        None
    }
}

/// 把一个目录的直接子目录压栈：读不了就跳过这一支
fn push_children(dir: &Path, prefix: &str, pending: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let key = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        pending.push((entry.path(), key));
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

    /// 要求 store 落点空闲并给出工作副本落点：落点上任何东西都报已存在。
    /// 这是落点闸而非占位判定——落点闸看存在性，回答「能不能往这放」；
    /// 键迁移把新键挪进来前必须先过这一闸
    pub fn require_free_store(&self) -> Result<PathBuf, StateError> {
        let to = self.paths.skill_workdir(&self.key);
        if occupancy(&to) != Occupancy::Nothing {
            return Err(StateError::AlreadyExists {
                key: self.key.clone(),
            });
        }
        Ok(to)
    }

    /// 要求归档侧空闲并给出归档落点：落点上任何东西都报冲突。
    /// 这是落点闸而非占位判定——落点闸看存在性，回答「能不能往这放」；
    /// 落点被占着仍硬移过去会静默替换掉用户文件
    pub fn require_free_archive(&self) -> Result<PathBuf, StateError> {
        let to = self.paths.skill_archived(&self.key);
        if occupancy(&to) != Occupancy::Nothing {
            return Err(StateError::Conflict {
                key: self.key.clone(),
            });
        }
        Ok(to)
    }

    /// 恢复的两端：归档副本必须在场、store 落点必须空闲。
    /// store 落点被任何东西占着报冲突（落点闸看存在性），归档副本不在场报不存在
    pub fn require_restorable(&self) -> Result<RestoreEnds, StateError> {
        let to = self.paths.skill_workdir(&self.key);
        if occupancy(&to) != Occupancy::Nothing {
            return Err(StateError::Conflict {
                key: self.key.clone(),
            });
        }
        match &self.archive {
            Some(from) => Ok(RestoreEnds {
                from: from.clone(),
                to,
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
