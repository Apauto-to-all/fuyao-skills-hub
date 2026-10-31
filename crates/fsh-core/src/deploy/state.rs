//! 漂移检查：项目侧（链接 / copy 哈希 / 版本存在性）与
//! hub 侧（工作副本 / 物化目录）的双侧状态核对。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::DeployError;
use super::model::{hash_dir_contents, hash_tag_contents, read_lock};
use super::open_repo;
use crate::git;
use crate::link::LinkKind;
use crate::paths::HubPaths;

// ---------- status 的输出：漂移清单（可序列化） ----------

/// 单条漂移发现
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "drift", rename_all = "snake_case")]
pub enum Drift {
    /// 项目侧：链接落点不存在、悬空，或被外部重指到别的目录
    LinkMissing {
        /// 技能名
        name: String,
    },
    /// 项目侧：copy 模式内容被改动（哈希与账本不符）
    CopyModified {
        /// 技能名
        name: String,
    },
    /// 项目侧：账本引用的版本在 hub 中已不存在
    VersionGone {
        /// 技能名
        name: String,
        /// 消失的版本
        version: String,
    },
    /// hub 侧：工作副本有未提交改动
    HubDirty,
    /// hub 侧：物化目录内容与版本 tag 不符
    MaterialMismatch {
        /// 技能名
        name: String,
        /// 受疑版本
        version: String,
    },
}

/// 双侧漂移检测：项目侧（链接/copy 哈希/版本存在性）+ hub 侧（工作副本/
/// 物化目录）。零漂移返回空表。
pub fn status(hub_root: &Path, project_dir: &Path) -> Result<Vec<Drift>, DeployError> {
    let repo = open_repo(hub_root)?;
    let lock = read_lock(project_dir)?;
    let mut drifts = Vec::new();

    for entry in &lock.entries {
        // 项目侧：链接存在且可达
        let dst = project_dir.join(&entry.dst);
        let present = fs::symlink_metadata(&dst).is_ok() && dst.exists();
        if !present {
            drifts.push(Drift::LinkMissing {
                name: entry.name.clone(),
            });
            continue;
        }
        // 项目侧：copy 模式内容被改
        if entry.kind == LinkKind::Copy
            && hash_dir_contents(&dst).ok().as_deref() != Some(entry.tree_hash.as_str())
        {
            drifts.push(Drift::CopyModified {
                name: entry.name.clone(),
            });
        }
    }

    // hub 侧：工作副本脏
    if git::is_dirty(&repo)? {
        drifts.push(Drift::HubDirty);
    }

    let install_dir = hub_root
        .parent()
        .ok_or_else(|| DeployError::HubNotInitialized(hub_root.display().to_string()))?;
    let paths = HubPaths::new(install_dir);
    for entry in &lock.entries {
        let tag = format!("{}/v{}", entry.name, entry.version);
        // 项目侧补充：账本版本在 hub 中已消失
        let version_exists = !matches!(
            git::resolve(&repo, &tag),
            Err(git::GitError::TagNotFound { .. })
        );
        if !version_exists {
            drifts.push(Drift::VersionGone {
                name: entry.name.clone(),
                version: entry.version.clone(),
            });
            continue;
        }
        // hub 侧：物化目录缺失或与 tag 不符
        let mat = paths.materialized(&entry.name, &entry.version);
        // 项目侧补充：reparse 链接（symlink/junction）被外部重指到
        // 别的目录——copy 有 hash 兜底，链接模式的指向必须显式校验
        if entry.kind != LinkKind::Copy {
            let dst = project_dir.join(&entry.dst);
            if matches!((dst.canonicalize(), mat.canonicalize()), (Ok(a), Ok(b)) if a != b) {
                drifts.push(Drift::LinkMissing {
                    name: entry.name.clone(),
                });
            }
        }
        let tag_hash = hash_tag_contents(&repo, &tag, &format!("store/{}", entry.name))?;
        if !mat.is_dir() || hash_dir_contents(&mat).ok().as_deref() != Some(tag_hash.as_str()) {
            drifts.push(Drift::MaterialMismatch {
                name: entry.name.clone(),
                version: entry.version.clone(),
            });
        }
    }
    Ok(drifts)
}
