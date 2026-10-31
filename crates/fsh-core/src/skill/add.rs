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
/// 名字唯一来源是 SKILL.md frontmatter；首版 = frontmatter version 缺省 0.1.0。
pub fn add(hub_root: &Path, src_dir: &Path) -> Result<AddReport, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let (name, version) = prepare_add(&repo, &paths, src_dir)?;

    copy_skill_tree(src_dir, &paths.skill_workdir(&name))?;
    let meta = SkillMeta {
        source: src_dir.display().to_string(),
        ..Default::default()
    };
    fs::write(meta_path(&paths, &name), toml::to_string(&meta)?)?;
    release(
        &repo,
        &paths,
        &name,
        &version,
        &format!("add {name} v{version}"),
        &_token,
    )?;
    Ok(AddReport {
        skill: name,
        version,
    })
}

/// add 预检：读身份、定首版、查重名；执行与 dry-run 共用同一套判定
fn prepare_add(
    repo: &git2::Repository,
    paths: &HubPaths,
    src_dir: &Path,
) -> Result<(String, String), SkillError> {
    let skill_md = src_dir.join("SKILL.md");
    let text = fs::read_to_string(&skill_md).map_err(|e| SkillError::InvalidSource {
        path: src_dir.display().to_string(),
        reason: format!("读不到 SKILL.md: {e}"),
    })?;
    let fm = skill_identity(&text)?;
    let version = fm.version.clone().unwrap_or_else(|| "0.1.0".to_string());
    if !valid_semver(&version) {
        return Err(SkillError::InvalidSource {
            path: src_dir.display().to_string(),
            reason: format!("frontmatter version 非法: {version}"),
        });
    }
    // 重名拒绝：工作副本、meta、版本 tag 任一占位都不允许覆盖
    if workdir_exists(paths, &fm.name)
        || meta_path(paths, &fm.name).exists()
        || !git::tag_list(repo, &fm.name)?.is_empty()
    {
        return Err(SkillError::AlreadyExists { skill: fm.name });
    }
    Ok((fm.name, version))
}

/// add 预演：与 add 同一套预检，返回将执行的动作清单，不落任何盘
pub fn add_dry(hub_root: &Path, src_dir: &Path) -> Result<Vec<String>, SkillError> {
    let (_token, repo, paths) = lock_and_open(hub_root)?;
    let (name, version) = prepare_add(&repo, &paths, src_dir)?;
    Ok(vec![
        format!(
            "copy {} -> {}",
            src_dir.display(),
            crate::naming::store_subpath(&name)
        ),
        format!("write meta/{name}.toml"),
        format!("commit: add {name} v{version}"),
        format!("tag {}", crate::naming::version_tag(&name, &version)),
        format!(
            "materialize {}/{}",
            crate::naming::store_subpath(&name),
            version
        ),
    ])
}
