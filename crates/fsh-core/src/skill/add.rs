//! 入库：本地 skill 目录一步到位成为库内实体（工作副本 + 自动提交）。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::frontmatter::parse;
use super::{SkillError, copy_skill_tree, ensure_name_matches_dir, skill_status};
use crate::commit::{Face, Verb, message};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep};
use crate::write::{self, Applied, OnFailure};

/// 预演载荷：预演标志置真，动作清单即计划里的步骤
impl From<Vec<PlanStep>> for AddOutcome {
    fn from(actions: Vec<PlanStep>) -> Self {
        Self {
            dry_run: true,
            actions,
            ..Self::default()
        }
    }
}

/// add 的结果载荷：实跑直出 `{key}`，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，实跑与预演两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 入库后的完整键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 将执行的步骤清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<PlanStep>,
}

/// 入库：本地 skill 目录 → 工作副本 + 机器提交。
/// 键末段唯一来源是 SKILL.md frontmatter 的 name（必须等于源目录名）；
/// key_prefix 提供键的组织前缀（如 `mattpocock/skills`），缺省入根下。
/// dry_run = true 时预演：只读不落盘、不取写锁——别人正在写也能拿到动作清单
pub fn add(
    hub_root: &Path,
    src_dir: &Path,
    key_prefix: Option<&str>,
    dry_run: bool,
) -> Result<AddOutcome, SkillError> {
    write::run(
        hub_root,
        dry_run,
        // 复制出的工作副本要撤干净：盘上不留没有入库记录的半棵树
        OnFailure::Rollback,
        |paths| prepare_add(paths, src_dir, key_prefix),
        |plan| AddOutcome::from(plan.actions),
        |ctx, plan| {
            // 先登记撤销再复制：复制中途失败时，已写下的半棵树同样要删净
            let undo = plan.dst.clone();
            let created = undo.clone();
            ctx.on_rollback(move || {
                if undo.exists()
                    && let Err(e) = fs::remove_dir_all(&undo)
                {
                    tracing::warn!(path = %undo.display(), cause = %e, "回滚工作副本删除失败");
                }
            });
            copy_skill_tree(&plan.src, &created)?;
            let commits = vec![message(Verb::Add, Face::Store, &plan.key)];
            Ok(Applied {
                outcome: AddOutcome {
                    key: plan.key.clone(),
                    ..AddOutcome::default()
                },
                commits,
                milestones: plan.key,
            })
        },
        |key, elapsed_ms| {
            // 入库跨复制目录与机器提交两段外部调用：一条 INFO 同时给出键与整段耗时
            tracing::info!(key = %key, elapsed_ms = elapsed_ms, "skill 入库完成");
        },
    )
}

/// add 预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct AddPlan {
    /// 源目录
    src: PathBuf,
    /// 入库落点：store 下的工作副本
    dst: PathBuf,
    /// 入库后的完整键
    key: String,
    /// 将执行的步骤清单
    actions: Vec<PlanStep>,
}

/// add 预检：读身份、校验键、查重键；动作清单在这里一次成形
fn prepare_add(
    paths: &HubPaths,
    src_dir: &Path,
    key_prefix: Option<&str>,
) -> Result<AddPlan, SkillError> {
    let skill_md = src_dir.join("SKILL.md");
    let text = fs::read_to_string(&skill_md).map_err(|e| SkillError::InvalidSource {
        path: src_dir.display().to_string(),
        reason: format!("读不到 SKILL.md: {e}"),
    })?;
    let fm = parse(&text, &skill_md.display().to_string())?;
    // name 必须等于源目录名：键末段与目录事实一致，生态递归发现才成立
    let dir_name = src_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    ensure_name_matches_dir(&fm, &dir_name, &src_dir.display().to_string())?;
    // 键段是被 git 与文件系统共同寻址的相对路径片段：形状规则由 key 模块统一判定
    crate::key::validate(&fm.name)?;
    let prefix = key_prefix.map(str::trim).filter(|p| !p.is_empty());
    if let Some(p) = prefix {
        // 前缀同样是键：同一套形状规则
        crate::key::validate(p)?;
    }
    let key = match prefix {
        Some(p) => format!("{p}/{}", fm.name),
        None => fm.name.clone(),
    };
    // 重键拒绝：键在场（工作副本或归档副本占位）都不允许覆盖
    if skill_status(paths, &key).is_present() {
        return Err(SkillError::AlreadyExists { key });
    }
    // 键不得长在别的键的内容树里：祖先段已是 skill 时拒绝——两棵目录树重叠
    // 会让键隔离失效
    let segments: Vec<&str> = key.split('/').collect();
    let mut ancestor = String::new();
    for seg in &segments[..segments.len().saturating_sub(1)] {
        if !ancestor.is_empty() {
            ancestor.push('/');
        }
        ancestor.push_str(seg);
        if paths.skill_workdir(&ancestor).join("SKILL.md").is_file() {
            return Err(SkillError::InvalidSource {
                path: src_dir.display().to_string(),
                reason: format!("键 {key} 落在已有键 {ancestor} 的内容树里"),
            });
        }
    }
    let actions = vec![
        PlanStep::between(
            ActionKind::Copy,
            src_dir.display().to_string(),
            crate::naming::store_subpath(&key),
        )
        .targeting(&key),
        PlanStep::commit(message(Verb::Add, Face::Store, &key)),
    ];
    Ok(AddPlan {
        src: src_dir.to_path_buf(),
        dst: paths.skill_workdir(&key),
        key,
        actions,
    })
}
