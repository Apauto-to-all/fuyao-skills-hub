//! 入库：本地 skill 目录一步到位成为库内实体（工作副本 + 自动提交）。

use std::fs;
use std::path::{Path, PathBuf};

use super::frontmatter::parse;
use super::outcome::{SkillOutcome, dry};
use super::{Frontmatter, SkillError, invalid, skill_occupancy};
use crate::commit::{Face, Verb, message};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep, Steps};
use crate::write::{self, Applied, OnFailure};

/// 入库：本地 skill 目录 → 工作副本 + 机器提交。
/// 键末段唯一来源是 SKILL.md frontmatter 的 name（必须等于源目录名）；
/// key_prefix 提供键的组织前缀（如 `mattpocock/skills`），缺省入根下。
/// dry_run = true 时预演：只读不落盘、不取写锁——别人正在写也能拿到动作清单
pub fn add(
    hub_root: &Path,
    src_dir: &Path,
    key_prefix: Option<&str>,
    dry_run: bool,
) -> Result<SkillOutcome, SkillError> {
    // 纯入参判定先于开仓：源目录身份与键形状都不依赖 hub，
    // 坏入参不落到「hub 未初始化」，预演与实跑同序
    let key = derive_key(src_dir, key_prefix)?;
    write::run(
        hub_root,
        dry_run,
        // 复制出的工作副本要撤干净：盘上不留没有入库记录的半棵树
        OnFailure::Rollback,
        |paths| prepare_add(paths, src_dir, &key),
        dry,
        |ctx, plan| {
            // 先登记撤销再复制：复制中途失败时，已写下的半棵树同样要删净
            let undo = plan.dst.clone();
            ctx.on_rollback(move || {
                if undo.exists()
                    && let Err(e) = fs::remove_dir_all(&undo)
                {
                    tracing::warn!(path = %undo.display(), cause = %e, "回滚工作副本删除失败");
                }
            });
            copy_skill_tree(&plan.src, &plan.dst)?;
            Ok(Applied {
                outcome: SkillOutcome {
                    key: key.clone(),
                    ..SkillOutcome::default()
                },
                milestones: key.clone(),
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
    /// 将执行的步骤清单
    actions: Vec<PlanStep>,
}

impl Steps for AddPlan {
    fn steps(&self) -> Vec<PlanStep> {
        self.actions.clone()
    }
}

/// 身份约束：frontmatter name 必须等于它所在目录名（键末段）。
/// 入库判据只有这一处，判断与文案只有这一份
fn ensure_name_matches_dir(
    frontmatter: &Frontmatter,
    dir_name: &str,
    label: &str,
) -> Result<(), SkillError> {
    if frontmatter.name == dir_name {
        return Ok(());
    }
    Err(invalid(
        label,
        format!(
            "frontmatter name ({}) 与所在目录名 ({dir_name}) 不一致",
            frontmatter.name
        ),
    ))
}

/// 递归复制目录树，跳过任何深度的 .git 目录（源自身是仓库时不带入嵌套仓库）
fn copy_skill_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_skill_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 纯入参段的键推导：读源目录身份并校验全部键段形状。
/// 只碰源目录与入参，不依赖 hub
fn derive_key(src_dir: &Path, key_prefix: Option<&str>) -> Result<String, SkillError> {
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
    Ok(match prefix {
        Some(p) => format!("{p}/{}", fm.name),
        None => fm.name,
    })
}

/// add 预检（hub 侧）：查重键、查键与既有内容树的冲突；动作清单在这里一次成形。
/// 源身份与键形状的入参判定住 [`derive_key`]，在入口先于开仓执行
fn prepare_add(paths: &HubPaths, src_dir: &Path, key: &str) -> Result<AddPlan, SkillError> {
    // 重键拒绝：键在场（工作副本或归档副本占位）都不允许覆盖
    skill_occupancy(paths, key).require_vacant()?;
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
            crate::naming::store_subpath(key),
        )
        .targeting(key),
        PlanStep::commit(message(Verb::Add, Face::Store, key)),
    ];
    Ok(AddPlan {
        src: src_dir.to_path_buf(),
        dst: paths.skill_workdir(key),
        actions,
    })
}
