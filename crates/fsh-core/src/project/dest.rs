//! 落点解析与落点守卫：agent 键 -> 实际落点、落点父目录检查与项目侧的内容比对。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::ProjectError;
use crate::agents::{self, AgentInfo};
use crate::identity::path_identity;
use crate::link::LinkKind;

/// 解析出的落点：实际目录与解析到它的 agent 键。
/// 路径持 `PathBuf`，三闸、建链接、删落点直接用它；只有报告出口才映射成字符串
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Dest {
    /// 实际落点绝对路径
    pub(super) path: PathBuf,
    /// 解析到该落点的 agent 键
    pub(super) agents: Vec<String>,
}

/// 项目落点：只用项目技能目录，只有全局位的 agent 到此为止
pub(super) fn resolve_project(
    known: &[AgentInfo],
    project: &Path,
    agent_keys: &[String],
    key: &str,
) -> Result<Vec<Dest>, ProjectError> {
    let destinations = resolve_with(known, agent_keys, key, |agent| {
        let skills_dir =
            agent
                .skills_dir
                .as_deref()
                .ok_or_else(|| agents::AgentError::NoProjectDir {
                    key: agent.key.clone(),
                })?;
        Ok(project.join(skills_dir))
    })?;
    // 目标路径计算是落点的唯一来源：DEBUG 记下项目与落点数，核对合并去重结果
    tracing::debug!(
        project = %project.display(),
        count = destinations.len(),
        "目标路径已解析"
    );
    Ok(destinations)
}

/// 全局落点：把配置里的全局技能目录原文展开成绝对路径，只有项目位的 agent 到此为止
pub(super) fn resolve_global(
    known: &[AgentInfo],
    agent_keys: &[String],
    key: &str,
) -> Result<Vec<Dest>, ProjectError> {
    let destinations =
        resolve_with(known, agent_keys, key, |agent| {
            let raw = agent.global_skills_dir.as_deref().ok_or_else(|| {
                agents::AgentError::NoGlobalDir {
                    key: agent.key.clone(),
                }
            })?;
            Ok(agents::expand_global(raw, agent.global_root, &agent.key)?)
        })?;
    // 全局目标没有项目根：DEBUG 只记落点数
    tracing::debug!(count = destinations.len(), "目标路径已解析");
    Ok(destinations)
}

/// 逐 agent 求技能目录、合并同一实际目录：落点 = 技能目录 + 键末段，
/// 共用目录的 agent 只留一条，保证一次使用只建一个入口。
/// 目录由调用方按作用域给出，缺目录的 agent 以错误点名
fn resolve_with(
    known: &[AgentInfo],
    agent_keys: &[String],
    key: &str,
    skills_dir: impl Fn(&AgentInfo) -> Result<PathBuf, ProjectError>,
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
        let dest = skills_dir(agent)?.join(crate::key::last_segment(key));
        grouped
            .entry(path_identity(&dest))
            .or_insert_with(|| Dest {
                path: dest,
                agents: Vec::new(),
            })
            .agents
            .push(agent_key.clone());
    }
    Ok(grouped.into_values().collect())
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
    let Ok(meta) = path.symlink_metadata() else {
        return Ok(DestVerdict::Missing);
    };
    match gates.kind {
        LinkKind::Copy => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(ProjectError::LinkKindMismatch {
                    path: path.display().to_string(),
                    expected: gates.kind,
                    actual: describe_kind(&meta),
                });
            }
            if gates.guard_copy && !dirs_equal(gates.source, path)? && !gates.force {
                return Err(ProjectError::ModifiedCopy {
                    path: path.display().to_string(),
                });
            }
        }
        LinkKind::Symlink | LinkKind::Junction => {
            if !meta.file_type().is_symlink() {
                return Err(ProjectError::LinkKindMismatch {
                    path: path.display().to_string(),
                    expected: gates.kind,
                    actual: describe_kind(&meta),
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
        if dir.symlink_metadata().is_ok() {
            if dir.is_dir() {
                return Ok(());
            }
            return Err(ProjectError::DestinationOccupied {
                path: dir.display().to_string(),
            });
        }
        cur = dir.parent();
    }
    Ok(())
}

/// 落点实际形态的人读描述（报错用）
pub(super) fn describe_kind(meta: &fs::Metadata) -> String {
    if meta.file_type().is_symlink() {
        "符号链接".to_string()
    } else if meta.is_dir() {
        "普通目录".to_string()
    } else {
        "普通文件".to_string()
    }
}

/// 两个目录内容是否一致：两侧各算一把同口径指纹再比。
/// 任一侧读不了即报错，由调用方据此判断，不静默当作一致
pub(super) fn dirs_equal(a: &Path, b: &Path) -> Result<bool, ProjectError> {
    Ok(crate::fingerprint::dir_digest(a)? == crate::fingerprint::dir_digest(b)?)
}
