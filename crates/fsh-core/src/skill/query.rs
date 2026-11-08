//! 查询：list 薄行清单、show 全量详情、diff 两版本差异。

use std::fs;
use std::path::Path;

use super::meta::read_meta;
use super::{
    DiffFile, SkillDetail, SkillError, SkillSummary, VersionEntry, open_repo, workdir_exists,
};
use crate::git;
use crate::hub;
use crate::paths::HubPaths;
use crate::project;

/// skill 全部唯一键：tag 命名空间 ∪ store 目录树
fn all_skill_keys(paths: &HubPaths, repo: &git::Repository) -> Result<Vec<String>, SkillError> {
    let mut keys: Vec<String> = git::tag_keys_all(repo)?;
    // store 侧：递归找含 SKILL.md 的目录；skill 内容树不再下钻（嵌套
    // skill 只可能出现在 key 前缀层，不会长在另一个 skill 的内容里）
    walk_store(&paths.store(), "", &mut |key| {
        if !keys.iter().any(|e| e == key) {
            keys.push(key.to_string());
        }
    });
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

/// 库清单：全部 skill 的薄行摘要（含归档态）；tag_filter 按 frontmatter tags 过滤
pub fn list(hub_root: &Path, tag_filter: Option<&str>) -> Result<Vec<SkillSummary>, SkillError> {
    let (repo, paths) = open_repo(hub_root)?;
    let mut out = Vec::new();
    for key in all_skill_keys(&paths, &repo)? {
        // 版本条目一次取全：最新版本号与发布提交时间同源
        let entries = git::tag_entries(&repo, &key)?;
        let versions: Vec<String> = entries.iter().map(|e| e.version.clone()).collect();
        // 来源、描述与标签只有 frontmatter 一处：活跃读工作副本，归档读最新 tag 树
        let (_, frontmatter) = read_meta(&repo, &paths, &key, &versions)?;
        if let Some(want) = tag_filter
            && !frontmatter
                .as_ref()
                .is_some_and(|f| f.tags.iter().any(|t| t == want))
        {
            continue;
        }
        let fm = frontmatter;
        let archived = !workdir_exists(&paths, &key);
        let updated = entries.first().map(|e| e.date);
        let description = fm.as_ref().map(|f| f.description.clone());
        let tags = fm.as_ref().map(|f| f.tags.clone()).unwrap_or_default();
        out.push(SkillSummary {
            latest_version: versions.first().cloned(),
            archived,
            source: fm.and_then(|f| f.source),
            description,
            tags,
            updated,
            key,
        });
    }
    // 查询命中数是判断过滤是否生效的唯一线索；只记总量，逐键明细不落日志
    tracing::debug!(count = out.len(), "skill 清单查询命中");
    Ok(out)
}

/// 单个 skill 全量详情：frontmatter 五字段、版本时间线、被引用项目
pub fn show(hub_root: &Path, key: &str) -> Result<SkillDetail, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    let (repo, paths) = open_repo(hub_root)?;
    let entries = git::tag_entries(&repo, key)?;
    let versions: Vec<String> = entries.iter().map(|e| e.version.clone()).collect();
    let archived = !workdir_exists(&paths, key);
    if versions.is_empty() && archived {
        return Err(SkillError::SkillNotFound { key: key.into() });
    }
    // 登记簿一次读全：引用项目与落点数同源
    let registry = hub::load_registry(&paths)?;
    // 落点解析依赖 agent 配置：读不出来时表达为未知（None），不冒充「零个落点」
    let destination_count = match project::destination_count(&paths, &registry, key) {
        Ok(count) => Some(count),
        Err(e) => {
            tracing::warn!(key = %key, cause = %e, "落点解析降级");
            None
        }
    };
    let (content, frontmatter) = read_meta(&repo, &paths, key, &versions)?;
    let timeline = entries
        .into_iter()
        .map(|entry| VersionEntry {
            version: entry.version,
            commit: entry.commit,
            date: entry.date,
        })
        .collect();
    Ok(SkillDetail {
        key: key.into(),
        latest_version: versions.first().cloned(),
        archived,
        frontmatter,
        destination_count,
        content,
        timeline,
        referenced_by: registry.referencing_projects(key),
        global_agents: registry.referencing_global_agents(key),
    })
}

/// 两版本差异：unified diff 按文件聚合
pub fn diff(hub_root: &Path, key: &str, v1: &str, v2: &str) -> Result<Vec<DiffFile>, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    let (repo, _paths) = open_repo(hub_root)?;
    let mut files: Vec<DiffFile> = Vec::new();
    let mut current: Option<(String, String)> = None;
    // 只保留 skill 子树内的路径并剥掉 store/<键>/ 前缀；
    // 树里其余部分（其他键、hub 自身文件）的变更不属于该 skill 的版本内容差异
    let prefix = format!("{}/", crate::naming::store_subpath(key));
    git::diff_tree_files(
        &repo,
        &crate::naming::version_tag(key, v1),
        &crate::naming::version_tag(key, v2),
        |delta, line| {
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
        },
    )?;
    if let Some(done) = current.take() {
        files.push(DiffFile {
            path: done.0,
            patch: done.1,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
