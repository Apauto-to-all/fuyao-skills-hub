//! 入库：本地 skill 目录一步到位成为库内实体
//! （工作副本 + meta + commit + 首版 tag + 物化）。

use std::fs;
use std::path::Path;

use super::{
    AddReport, SkillError, SkillMeta, copy_skill_tree, lock_and_open, meta_path, release,
    skill_identity, valid_semver, workdir_exists,
};
use crate::git;
use crate::paths::HubPaths;

/// 入库：本地 skill 目录 → 工作副本 + meta + commit + 首版 tag + 物化。
/// 键末段唯一来源是 SKILL.md frontmatter 的 name（必须等于源目录名）；
/// key_prefix 提供键的组织前缀（如 `anthropics/skills`），缺省入根下。
pub fn add(
    hub_root: &Path,
    src_dir: &Path,
    key_prefix: Option<&str>,
) -> Result<AddReport, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let (key, version) = prepare_add(&repo, &paths, src_dir, key_prefix)?;

    copy_skill_tree(src_dir, &paths.skill_workdir(&key))?;
    let meta = SkillMeta {
        source: src_dir.display().to_string(),
        ..Default::default()
    };
    if let Some(parent) = meta_path(&paths, &key).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(meta_path(&paths, &key), toml::to_string(&meta)?)?;
    release(
        &repo,
        &paths,
        &key,
        &version,
        &format!("add {key} v{version}"),
        &_token,
    )?;
    Ok(AddReport { key, version })
}

/// add 预检：读身份、校验键、定首版、查重键；执行与 dry-run 共用同一套判定
fn prepare_add(
    repo: &git2::Repository,
    paths: &HubPaths,
    src_dir: &Path,
    key_prefix: Option<&str>,
) -> Result<(String, String), SkillError> {
    let skill_md = src_dir.join("SKILL.md");
    let text = fs::read_to_string(&skill_md).map_err(|e| SkillError::InvalidSource {
        path: src_dir.display().to_string(),
        reason: format!("读不到 SKILL.md: {e}"),
    })?;
    let fm = skill_identity(&text)?;
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
    if crate::naming::is_version_like(&fm.name) {
        return Err(SkillError::InvalidSource {
            path: src_dir.display().to_string(),
            reason: format!(
                "目录名形如版本目录（三段数字），与物化布局冲突: {}",
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
                    || crate::naming::is_version_like(seg)
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
    let version = fm.version.clone().unwrap_or_else(|| "0.1.0".to_string());
    if !valid_semver(&version) {
        return Err(SkillError::InvalidSource {
            path: src_dir.display().to_string(),
            reason: format!("frontmatter version 非法: {version}"),
        });
    }
    // 重键拒绝：工作副本、meta、版本 tag 任一占位都不允许覆盖
    if workdir_exists(paths, &key)
        || meta_path(paths, &key).exists()
        || !git::tag_list(repo, &key)?.is_empty()
    {
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
    Ok((key, version))
}

/// add 预演：与 add 同一套预检，返回将执行的动作清单，不落任何盘
pub fn add_dry(
    hub_root: &Path,
    src_dir: &Path,
    key_prefix: Option<&str>,
) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let (key, version) = prepare_add(&repo, &paths, src_dir, key_prefix)?;
    Ok(vec![
        format!(
            "copy {} -> {}",
            src_dir.display(),
            crate::naming::store_subpath(&key)
        ),
        format!("write meta/{key}.toml"),
        format!("commit: add {key} v{version}"),
        format!("tag {}", crate::naming::version_tag(&key, &version)),
        format!(
            "materialize {}",
            crate::naming::materialized_subpath(&key, &version)
        ),
    ])
}
