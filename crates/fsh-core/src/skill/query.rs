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

/// skill 全部唯一键：tag 命名空间 ∪ store 目录树
fn all_skill_keys(paths: &HubPaths, repo: &git2::Repository) -> Result<Vec<String>, SkillError> {
    let mut keys: Vec<String> = Vec::new();
    // 去重压入：两侧来源（tag 与目录树）会给出同一键
    let remember = |key: &str, keys: &mut Vec<String>| {
        if !key.is_empty() && !keys.iter().any(|e| e == key) {
            keys.push(key.to_string());
        }
    };
    // tag 侧：尾段语义（v<semver> / stable）反解键，容忍多级键
    repo.tag_foreach(|_oid, name| {
        if let Some(key) = crate::naming::key_from_tag_ref(&String::from_utf8_lossy(name)) {
            remember(key, &mut keys);
        }
        true
    })?;
    // store 侧：递归找含 SKILL.md 的目录；skill 内容树不再下钻（嵌套
    // skill 只可能出现在 key 前缀层，不会长在另一个 skill 的内容里）
    walk_store(&paths.store(), "", &mut |key| remember(key, &mut keys));
    keys.sort();
    Ok(keys)
}

/// 递归走查 store：目录含 SKILL.md 即是一个键（相对路径），否则继续下钻
fn walk_store(dir: &Path, prefix: &str, found: &mut dyn FnMut(&str)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if p.join("SKILL.md").is_file() {
            found(&rel);
        } else {
            walk_store(&p, &rel, found);
        }
    }
}

/// 库清单：全部 skill 的薄行摘要（含归档态）；tag_filter 按 meta tags 过滤
pub fn list(hub_root: &Path, tag_filter: Option<&str>) -> Result<Vec<SkillSummary>, SkillError> {
    let repo = open_repo(hub_root)?;
    let paths = HubPaths::from_root(hub_root);
    let mut out = Vec::new();
    for key in all_skill_keys(&paths, &repo)? {
        let meta = load_meta(&paths, &key)?;
        if let Some(want) = tag_filter {
            let has = meta.as_ref().map(|m| m.tags.iter().any(|t| t == want));
            if has != Some(true) {
                continue;
            }
        }
        out.push(SkillSummary {
            latest_version: git::tag_list(&repo, &key)?.into_iter().next(),
            archived: !workdir_exists(&paths, &key),
            source: meta.map(|m| m.source),
            key,
        });
    }
    Ok(out)
}

/// 单个 skill 全量详情：meta、版本时间线、frontmatter 描述、被引用项目
pub fn show(hub_root: &Path, key: &str) -> Result<SkillDetail, SkillError> {
    let repo = open_repo(hub_root)?;
    let paths = HubPaths::from_root(hub_root);
    let versions = git::tag_list(&repo, key)?;
    let archived = !workdir_exists(&paths, key);
    if versions.is_empty() && archived {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    // frontmatter：活跃态读工作副本，归档态读最新 tag 里的 SKILL.md
    let work_md = paths.skill_workdir(key).join("SKILL.md");
    let description = if !archived {
        fs::read_to_string(&work_md)
            .ok()
            .and_then(|t| skill_identity(&t, &work_md.display().to_string()).ok())
            .map(|fm| fm.description)
    } else {
        let tag = crate::naming::version_tag(key, &versions[0]);
        let label = format!("{tag}:{}/SKILL.md", crate::naming::store_subpath(key));
        skill_md_from_tag(&repo, &tag, key)?
            .and_then(|t| skill_identity(&t, &label).ok())
            .map(|fm| fm.description)
    };
    Ok(SkillDetail {
        latest_version: versions.first().cloned(),
        archived,
        meta: load_meta(&paths, key)?,
        versions,
        description,
        referenced_by: referencing_projects(&paths, key)?,
        key: key.into(),
    })
}

/// 两版本差异：unified diff 按文件聚合
pub fn diff(hub_root: &Path, key: &str, v1: &str, v2: &str) -> Result<Vec<DiffFile>, SkillError> {
    let repo = open_repo(hub_root)?;
    let (_, t1) = git::resolve(&repo, &crate::naming::version_tag(key, v1))?;
    let (_, t2) = git::resolve(&repo, &crate::naming::version_tag(key, v2))?;
    let tree1 = repo.find_tree(t1)?;
    let tree2 = repo.find_tree(t2)?;
    let diff = repo.diff_tree_to_tree(Some(&tree1), Some(&tree2), None)?;

    let mut files: Vec<DiffFile> = Vec::new();
    let mut current: Option<(String, String)> = None;
    // 只保留 skill 子树内的路径并剥掉 store/<键>/ 前缀；
    // 树里其余部分（meta 等）的变更不属于该 skill 的版本内容差异
    let prefix = format!("{}/", crate::naming::store_subpath(key));
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
