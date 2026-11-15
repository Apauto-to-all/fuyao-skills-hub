//! 落点解析与落点守卫：agent 键 -> 实际落点、落点父目录检查与项目侧的内容比对。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Anchor, ProjectError};
use crate::agents::{self, AgentInfo};
use crate::identity::path_identity;
use crate::link::LinkKind;
use crate::skill::{Occupancy, occupancy};

/// 解析出的落点：实际目录与解析到它的 agent 键。
/// 路径持 `PathBuf`，三闸、建链接、删落点直接用它；只有报告出口才映射成字符串
#[derive(Debug, Clone)]
pub(crate) struct Dest {
    /// 实际落点绝对路径
    pub(crate) path: PathBuf,
    /// 解析到该落点的 agent 键
    pub(crate) agents: Vec<String>,
}

impl Anchor {
    /// 本目标的落点：项目目标 = 项目根 + agent 的技能目录，全局目标 = 加载时展开好的
    /// 全局目录；两者都再拼键末段。共用同一实际目录的 agent 只留一条，
    /// 保证一次使用只建一个入口；该作用域没有目录的 agent 以错误点名
    pub(super) fn resolve(
        &self,
        known: &[AgentInfo],
        agent_keys: &[String],
        key: &str,
    ) -> Result<Vec<Dest>, ProjectError> {
        let mut grouped: BTreeMap<String, Dest> = BTreeMap::new();
        for agent_key in agent_keys {
            let agent = known
                .iter()
                .find(|agent| agent.key == *agent_key)
                .ok_or_else(|| agents::AgentError::UnknownAgent {
                    key: agent_key.clone(),
                    known: known
                        .iter()
                        .map(|agent| agent.key.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                })?;
            let skills_dir = match self {
                Anchor::Project(dir) => {
                    let skills_dir = agent.skills_dir.as_deref().ok_or_else(|| {
                        agents::AgentError::NoProjectDir {
                            key: agent.key.clone(),
                        }
                    })?;
                    dir.join(skills_dir)
                }
                Anchor::Global => {
                    agent
                        .global_dir
                        .clone()
                        .ok_or_else(|| agents::AgentError::NoGlobalDir {
                            key: agent.key.clone(),
                        })?
                }
            };
            let dest = skills_dir.join(crate::key::last_segment(key));
            grouped
                .entry(path_identity(&dest))
                .or_insert_with(|| Dest {
                    path: dest,
                    agents: Vec::new(),
                })
                .agents
                .push(agent_key.clone());
        }
        // 目标路径计算是落点的唯一来源：DEBUG 记下点数，核对合并去重结果
        let count = grouped.len();
        match self.project() {
            Some(project) => tracing::debug!(project = %project, count = count, "目标路径已解析"),
            None => tracing::debug!(count = count, "目标路径已解析"),
        }
        Ok(grouped.into_values().collect())
    }
}

/// 落点三闸的核对结论
pub(super) enum DestVerdict {
    /// 落点在盘上，形态与靶子都对得上
    Live,
    /// 落点已不存在
    Missing,
}

/// 落点三闸的口径：登记档位、登记推导出的源、要不要比对拷贝内容、要不要放行手改拷贝
pub(super) struct DestGates<'a> {
    /// 登记簿里记录的链接方式
    pub kind: LinkKind,
    /// 落点靶子：store 工作副本
    pub source: &'a Path,
    /// 是否比对拷贝内容：不动盘的核对（幂等命中）不该被拷贝指纹误伤
    pub guard_copy: bool,
    /// 只覆盖「拷贝内容被修改」这一道，不覆盖形态与靶子
    pub force: bool,
}

/// 单个落点的三闸核对：形态与登记的档位一致、链接靶子仍是记录推导出的源、
/// 拷贝未被修改。落点已不存在时报 Missing，由调用方决定是跳过还是点名
pub(super) fn check_destination(
    dest: &Dest,
    gates: &DestGates<'_>,
) -> Result<DestVerdict, ProjectError> {
    let path = dest.path.as_path();
    let occupied = occupancy(path);
    if occupied == Occupancy::Nothing {
        return Ok(DestVerdict::Missing);
    }
    match gates.kind {
        LinkKind::Copy => {
            if occupied != Occupancy::Dir {
                return Err(ProjectError::LinkKindMismatch {
                    path: path.display().to_string(),
                    expected: gates.kind,
                    actual: occupied.to_string(),
                });
            }
            if gates.guard_copy && !dirs_equal(gates.source, path)? && !gates.force {
                return Err(ProjectError::ModifiedCopy {
                    path: path.display().to_string(),
                });
            }
        }
        LinkKind::Symlink | LinkKind::Junction => {
            if occupied != Occupancy::Link {
                return Err(ProjectError::LinkKindMismatch {
                    path: path.display().to_string(),
                    expected: gates.kind,
                    actual: occupied.to_string(),
                });
            }
            // 链接指向也必须仍是这条记录推导出的源，换了靶子就不是本工具建的入口
            let target_ok = fs::canonicalize(path)
                .ok()
                .zip(fs::canonicalize(gates.source).ok())
                .is_some_and(|(a, b)| path_identity(&a) == path_identity(&b));
            if !target_ok {
                return Err(ProjectError::LinkKindMismatch {
                    path: path.display().to_string(),
                    expected: gates.kind,
                    actual: "链接目标已改变".to_string(),
                });
            }
        }
    }
    Ok(DestVerdict::Live)
}

/// 落点父目录链上最近的存在项必须是目录：否则建链接会在中途失败。
/// 一路上没有存在项就一直走到盘根（盘根必是目录，循环自然收束）
pub(super) fn ensure_parent_is_dir(dest: &Path) -> Result<(), ProjectError> {
    let mut cur = dest.parent();
    while let Some(dir) = cur {
        match occupancy(dir) {
            Occupancy::Nothing => cur = dir.parent(),
            Occupancy::Dir => return Ok(()),
            // 链接指向目录时照样能当父目录：建链接只要求中间段可穿行
            Occupancy::Link if dir.is_dir() => return Ok(()),
            occupied => {
                return Err(ProjectError::DestinationOccupied {
                    path: dir.display().to_string(),
                    kind: occupied.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// 两个目录内容是否一致：两侧各算一把同口径指纹再比。
/// 任一侧读不了即报错，由调用方据此判断，不静默当作一致
pub(super) fn dirs_equal(a: &Path, b: &Path) -> Result<bool, ProjectError> {
    Ok(crate::fingerprint::dir_digest(a)? == crate::fingerprint::dir_digest(b)?)
}
