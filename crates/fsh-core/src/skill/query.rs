//! 查询：list 薄行清单、show 全量详情、diff 两版本差异。

use std::fs;
use std::path::Path;

use git2::DiffFormat;

use super::{
    DiffFile, SkillDetail, SkillError, SkillSummary, load_meta, open_repo, referencing_projects,
    skill_identity, skill_md_from_tag, workdir_exists,
};
use crate::git;
use crate::paths::HubPaths;

/// skill 全部名字：tag 命名空间 ∪ store 目录
fn all_skill_names(paths: &HubPaths, repo: &git2::Repository) -> Result<Vec<String>, SkillError> {
    let mut names: Vec<String> = Vec::new();
    let push = |n: &str, names: &mut Vec<String>| {
        if !n.is_empty() && !names.iter().any(|e| e == n) {
            names.push(n.to_string());
        }
    };
    repo.tag_foreach(|_oid, name| {
        if let Some(short) = String::from_utf8_lossy(name).strip_prefix("refs/tags/") {
            // 命名空间形如 <skill>/v... 或 <skill>/stable，取 / 前一段
            if let Some(ns) = short.split_once('/') {
                push(ns.0, &mut names);
            }
        }
        true
    })?;
    if let Ok(entries) = fs::read_dir(paths.store()) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                push(&e.file_name().to_string_lossy(), &mut names);
            }
        }
    }
    names.sort();
    Ok(names)
}

/// 库清单：全部 skill 的薄行摘要（含归档态）；tag_filter 按 meta tags 过滤
pub fn list(hub_root: &Path, tag_filter: Option<&str>) -> Result<Vec<SkillSummary>, SkillError> {
    let repo = open_repo(hub_root)?;
    let paths = HubPaths::from_root(hub_root);
    let mut out = Vec::new();
    for name in all_skill_names(&paths, &repo)? {
        let meta = load_meta(&paths, &name)?;
        if let Some(want) = tag_filter {
            let has = meta.as_ref().map(|m| m.tags.iter().any(|t| t == want));
            if has != Some(true) {
                continue;
            }
        }
        out.push(SkillSummary {
            latest_version: git::tag_list(&repo, &name)?.into_iter().next(),
            archived: !workdir_exists(&paths, &name),
            source: meta.map(|m| m.source),
            name,
        });
    }
    Ok(out)
}

/// 单个 skill 全量详情：meta、版本时间线、frontmatter 描述、被引用项目
pub fn show(hub_root: &Path, skill: &str) -> Result<SkillDetail, SkillError> {
    let repo = open_repo(hub_root)?;
    let paths = HubPaths::from_root(hub_root);
    let versions = git::tag_list(&repo, skill)?;
    let archived = !workdir_exists(&paths, skill);
    if versions.is_empty() && archived {
        return Err(SkillError::SkillNotFound {
            skill: skill.into(),
        });
    }
    // frontmatter：活跃态读工作副本，归档态读最新 tag 里的 SKILL.md
    let description = if !archived {
        fs::read_to_string(paths.skill_workdir(skill).join("SKILL.md"))
            .ok()
            .and_then(|t| skill_identity(&t).ok())
            .and_then(|fm| fm.description)
    } else {
        skill_md_from_tag(&repo, &format!("{skill}/v{}", versions[0]), skill)?
            .and_then(|t| skill_identity(&t).ok())
            .and_then(|fm| fm.description)
    };
    Ok(SkillDetail {
        latest_version: versions.first().cloned(),
        archived,
        meta: load_meta(&paths, skill)?,
        versions,
        description,
        referenced_by: referencing_projects(&paths, skill)?,
        name: skill.into(),
    })
}

/// 两版本差异：unified diff 按文件聚合
pub fn diff(hub_root: &Path, skill: &str, v1: &str, v2: &str) -> Result<Vec<DiffFile>, SkillError> {
    let repo = open_repo(hub_root)?;
    let (_, t1) = git::resolve(&repo, &format!("{skill}/v{v1}"))?;
    let (_, t2) = git::resolve(&repo, &format!("{skill}/v{v2}"))?;
    let tree1 = repo.find_tree(t1)?;
    let tree2 = repo.find_tree(t2)?;
    let diff = repo.diff_tree_to_tree(Some(&tree1), Some(&tree2), None)?;

    let mut files: Vec<DiffFile> = Vec::new();
    let mut current: Option<(String, String)> = None;
    // 只保留 skill 子树内的路径并剥掉 store/<skill>/ 前缀；
    // 树里其余部分（meta 等）的变更不属于该 skill 的版本内容差异
    let prefix = format!("store/{skill}/");
    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        // delta 非空表示进入新文件：把上一份落盘，开新一份
        if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
            let path = path.to_string_lossy().into_owned();
            // 子树外的变更整份跳过：先落盘上一份，当前置空（不再累积内容行）
            let rel = path.strip_prefix(&prefix).map(str::to_string);
            // 同一文件的后续内容行不动缓冲；换文件（或出子树）才落盘重开
            let same_file = matches!((&current, &rel), (Some((p, _)), Some(r)) if p == r);
            if !same_file {
                if let Some(done) = current.take() {
                    files.push(DiffFile {
                        path: done.0,
                        patch: done.1,
                    });
                }
                if let Some(r) = rel {
                    current = Some((r, String::new()));
                }
            }
        }
        if let Some((_, buf)) = current.as_mut() {
            // 内容行的 +/-/空格 前缀在 origin 里，须手动拼回才是 unified diff 文本；
            // 文件头（F）与 hunk 头（H）行的 content 自身已完整
            match line.origin() {
                '+' | '-' | ' ' => {
                    buf.push(line.origin());
                    buf.push_str(&String::from_utf8_lossy(line.content()));
                }
                _ => buf.push_str(&String::from_utf8_lossy(line.content())),
            }
        }
        true
    })?;
    if let Some(done) = current.take() {
        files.push(DiffFile {
            path: done.0,
            patch: done.1,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
