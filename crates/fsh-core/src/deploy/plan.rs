//! 变更清单：对比声明、账本与 hub 现状，纯计算无副作用。

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::DeployError;
use super::model::{SkillReq, read_lock, read_manifest};
use super::open_repo;
use crate::git;

// ---------- plan 的输出：变更清单（可序列化，GUI 直接渲染） ----------

/// 单条计划变更
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Change {
    /// 新装：hub 有此技能此版本，项目还没链
    Install {
        /// 技能名
        name: String,
        /// 解析后的精确版本
        version: String,
        /// 目标工具名
        target: String,
    },
    /// 升级到更高版本
    Upgrade {
        /// 技能名
        name: String,
        /// 旧版本
        from: String,
        /// 新版本
        to: String,
        /// 目标工具名
        target: String,
    },
    /// 降级到更低版本
    Downgrade {
        /// 技能名
        name: String,
        /// 旧版本
        from: String,
        /// 新版本
        to: String,
        /// 目标工具名
        target: String,
    },
    /// 声明中移除：撤销链接并销账
    Remove {
        /// 技能名
        name: String,
        /// 将被撤销的版本
        version: String,
    },
    /// 已满足：无需任何动作
    Noop {
        /// 技能名
        name: String,
        /// 已就位的版本
        version: String,
    },
}

/// 把声明里的版本引用解析为精确版本：stable → 最新版本 tag；
/// 精确版本需在 hub 中真实存在
fn resolve_version(repo: &git2::Repository, req: &SkillReq) -> Result<String, DeployError> {
    if req.version == "stable" {
        return git::tag_list(repo, &req.name)?.into_iter().next().ok_or(
            DeployError::VersionNotFound {
                skill: req.name.clone(),
                version: req.version.clone(),
            },
        );
    }
    let tag = crate::naming::version_tag(&req.name, &req.version);
    match git::resolve(repo, &tag) {
        Ok(_) => Ok(req.version.clone()),
        Err(git::GitError::TagNotFound { .. }) => Err(DeployError::VersionNotFound {
            skill: req.name.clone(),
            version: req.version.clone(),
        }),
        Err(e) => Err(e.into()),
    }
}

/// 计算变更清单：对比 skills.toml 声明、skills.lock.toml 账本与 hub 现状。
/// 纯计算，不产生任何磁盘副作用。
pub fn plan(hub_root: &Path, project_dir: &Path) -> Result<Vec<Change>, DeployError> {
    let repo = open_repo(hub_root)?;
    let manifest = read_manifest(project_dir)?;
    let lock = read_lock(project_dir)?;

    let mut changes = Vec::new();
    // 声明侧：逐条对比账本
    for req in &manifest.skills {
        let version = resolve_version(&repo, req)?;
        match lock.entries.iter().find(|e| e.name == req.name) {
            None => changes.push(Change::Install {
                name: req.name.clone(),
                version,
                target: req.target.clone(),
            }),
            Some(entry) => {
                if entry.version == version {
                    changes.push(Change::Noop {
                        name: req.name.clone(),
                        version,
                    });
                } else {
                    // 新旧版本在 tag 降序表中的位置决定升降级语义
                    let order = git::tag_list(&repo, &req.name)?;
                    let from_pos = order.iter().position(|v| v == &entry.version);
                    let to_pos = order.iter().position(|v| v == &version);
                    let is_upgrade = match (from_pos, to_pos) {
                        (Some(f), Some(t)) => t < f,
                        // 位置对不上时按升级处理（账本版本可能已不在 hub）
                        _ => true,
                    };
                    let fields = (
                        req.name.clone(),
                        entry.version.clone(),
                        version,
                        req.target.clone(),
                    );
                    changes.push(if is_upgrade {
                        Change::Upgrade {
                            name: fields.0,
                            from: fields.1,
                            to: fields.2,
                            target: fields.3,
                        }
                    } else {
                        Change::Downgrade {
                            name: fields.0,
                            from: fields.1,
                            to: fields.2,
                            target: fields.3,
                        }
                    });
                }
            }
        }
    }
    // 账本侧：声明里没有的技能要撤销
    for entry in &lock.entries {
        if !manifest.skills.iter().any(|r| r.name == entry.name) {
            changes.push(Change::Remove {
                name: entry.name.clone(),
                version: entry.version.clone(),
            });
        }
    }
    Ok(changes)
}
