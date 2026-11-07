//! 入库：本地 skill 目录一步到位成为库内实体
//! （工作副本 + commit + 首版 tag）。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::frontmatter::parse;
use super::{
    SkillError, copy_skill_tree, lock_and_open, open_repo, publish, set_frontmatter_version,
    workdir_exists,
};
use crate::git;
use crate::paths::HubPaths;

/// add 的结果载荷：实跑直出 `{key, version}`，预演直出 `{dry_run, actions}`；
/// 缺席字段经 serde 跳过，实跑与预演两种信封共用同一形状
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddOutcome {
    /// 预演标志（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub dry_run: bool,
    /// 入库后的完整键（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub key: String,
    /// 首版版本号（仅实跑出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub version: String,
    /// 将执行的动作清单（仅预演出现）
    #[serde(skip_serializing_if = "crate::is_default")]
    pub actions: Vec<String>,
}

/// 入库：本地 skill 目录 → 工作副本 + commit + 首版 tag。
/// 键末段唯一来源是 SKILL.md frontmatter 的 name（必须等于源目录名）；
/// key_prefix 提供键的组织前缀（如 `mattpocock/skills`），缺省入根下。
/// dry_run = true 时预演：只读不落盘、不取写锁——别人正在写也能拿到动作清单
pub fn add(
    hub_root: &Path,
    src_dir: &Path,
    key_prefix: Option<&str>,
    dry_run: bool,
) -> Result<AddOutcome, SkillError> {
    if dry_run {
        let (repo, paths) = open_repo(hub_root)?;
        let plan = prepare_add(&repo, &paths, src_dir, key_prefix)?;
        return Ok(AddOutcome {
            dry_run: true,
            actions: plan.actions,
            ..AddOutcome::default()
        });
    }
    let started = std::time::Instant::now();
    let (token, repo, paths) = lock_and_open(hub_root)?;
    let plan = prepare_add(&repo, &paths, src_dir, key_prefix)?;

    copy_skill_tree(src_dir, &paths.skill_workdir(&plan.key))?;
    // 源目录没写号时把首版号写进编辑面：号只有一个诞生地（文件），
    // 入库后 `fsh show` / `fsh release` 才读得到；源里已写号的连格式一起原样保留
    if let Some(text) = plan.rewrite {
        fs::write(paths.skill_workdir(&plan.key).join("SKILL.md"), text)?;
    }
    publish(
        &repo,
        &paths,
        &plan.key,
        &plan.version,
        &format!("add {} v{}", plan.key, plan.version),
        &token,
    )?;
    // 入库跨复制目录、提交、打标三次外部调用：一条 INFO 同时给出键、首版号与整段耗时
    tracing::info!(
        key = %plan.key,
        version = %plan.version,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "skill 入库完成"
    );
    Ok(AddOutcome {
        key: plan.key,
        version: plan.version,
        ..AddOutcome::default()
    })
}

/// add 预检结果：实跑照它执行，预演直接列它，清单只有这一处产出
struct AddPlan {
    /// 入库后的完整键
    key: String,
    /// 首版版本号
    version: String,
    /// 源目录缺 version 时补好号的新 frontmatter 文本
    rewrite: Option<String>,
    /// 将执行的动作清单
    actions: Vec<String>,
}

/// add 预检：读身份、校验键、定首版、查重键；动作清单在这里一次成形
fn prepare_add(
    repo: &git::Repository,
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
    if fm.name != dir_name {
        return Err(SkillError::InvalidSource {
            path: src_dir.display().to_string(),
            reason: format!(
                "frontmatter name ({}) 与所在目录名 ({dir_name}) 不一致",
                fm.name
            ),
        });
    }
    // 键段是被 git 与文件系统共同寻址的相对路径片段：禁反斜杠、空段、
    // 盘符或绝对起点、`.`/`..`（会越出 hub），也禁三段纯数字（与 gc 的
    // 版本目录判定撞形会生出歧义）
    if crate::version::Version::parse(&fm.name).is_some() {
        return Err(SkillError::InvalidSource {
            path: src_dir.display().to_string(),
            reason: format!(
                "目录名形如版本目录（三段数字），与导出目录布局冲突: {}",
                fm.name
            ),
        });
    }
    let prefix = key_prefix.map(str::trim).filter(|p| !p.is_empty());
    if let Some(p) = prefix
        && (p.contains('\\')
            || p.split('/').any(|seg| {
                let seg = seg.trim();
                seg.is_empty()
                    || seg == "."
                    || seg == ".."
                    || seg.contains(':')
                    || crate::version::Version::parse(seg).is_some()
            }))
    {
        return Err(SkillError::InvalidSource {
            path: src_dir.display().to_string(),
            reason: format!("键前缀非法（须为不含空段/盘符/`..`/版本目录段的正斜杠路径）: {p}"),
        });
    }
    let key = match prefix {
        Some(p) => format!("{p}/{}", fm.name),
        None => fm.name.clone(),
    };
    // 号由文件定；缺省首版 0.1.0（格式已在 frontmatter 解析处校验）
    let version = fm.version.clone().unwrap_or_else(|| "0.1.0".to_string());
    // 重键拒绝：工作副本、版本 tag 任一占位都不允许覆盖
    if workdir_exists(paths, &key) || !git::tag_list(repo, &key)?.is_empty() {
        return Err(SkillError::AlreadyExists { key });
    }
    // 键不得长在别的键的内容树里：祖先段已是 skill（有工作副本或有
    // tag，含归档态）时拒绝——两棵版本树重叠会让版本隔离失效
    let segments: Vec<&str> = key.split('/').collect();
    let mut ancestor = String::new();
    for seg in &segments[..segments.len().saturating_sub(1)] {
        if !ancestor.is_empty() {
            ancestor.push('/');
        }
        ancestor.push_str(seg);
        if paths.skill_workdir(&ancestor).join("SKILL.md").is_file()
            || !git::tag_list(repo, &ancestor)?.is_empty()
        {
            return Err(SkillError::InvalidSource {
                path: src_dir.display().to_string(),
                reason: format!("键 {key} 落在已有键 {ancestor} 的内容树里"),
            });
        }
    }
    // 号由文件定，没写号的源算好首版后要把号落回文件
    let rewrite = match fm.version {
        Some(_) => None,
        None => Some(set_frontmatter_version(&text, &version)?),
    };
    let mut actions = vec![format!(
        "copy {} -> {}",
        src_dir.display(),
        crate::naming::store_subpath(&key)
    )];
    // 源里没写号时真 add 会把首版号写回编辑面，预演要如实列出这一步
    if rewrite.is_some() {
        actions.push(format!("写回 frontmatter version = {version}"));
    }
    actions.push(format!("commit: add {key} v{version}"));
    actions.push(format!(
        "tag {}",
        crate::naming::version_tag(&key, &version)
    ));
    Ok(AddPlan {
        key,
        version,
        rewrite,
        actions,
    })
}
