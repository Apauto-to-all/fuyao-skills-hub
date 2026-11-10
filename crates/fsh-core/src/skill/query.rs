//! 查询：show 全量详情、diff 两版本差异。

use std::path::Path;

use super::meta::read_meta;
use super::{DiffFile, SkillDetail, SkillError, SkillState, VersionEntry, open_repo, skill_status};
use crate::git;
use crate::hub;
use crate::project;

/// 单个 skill 全量详情：frontmatter 五字段、版本时间线、使用记录
pub fn show(hub_root: &Path, key: &str) -> Result<SkillDetail, SkillError> {
    // 键形状先判：写错的键当场点名，不落到「技能不存在」
    crate::key::validate(key)?;
    let (repo, paths) = open_repo(hub_root)?;
    let status = skill_status(&repo, &paths, key)?;
    status.require_present()?;
    // 登记簿一次读全：使用记录与落点数同源
    let registry = hub::load_registry(&paths)?;
    // 落点解析依赖 agent 配置：读不出来时表达为未知（None），不冒充「零个落点」
    let destination_count = match project::destination_count(&paths, &registry, key) {
        Ok(count) => Some(count),
        Err(e) => {
            tracing::warn!(key = %key, cause = %e, "落点解析降级");
            None
        }
    };
    let meta = read_meta(&repo, &paths, &status)?;
    let timeline = status
        .entries()
        .iter()
        .map(|entry| VersionEntry {
            version: entry.version.clone(),
            commit: entry.commit.clone(),
            date: entry.date,
        })
        .collect();
    Ok(SkillDetail {
        key: key.into(),
        latest_version: status.latest().map(str::to_string),
        archived: status.state() == SkillState::Archived,
        frontmatter: meta.frontmatter,
        frontmatter_error: meta.parse_error,
        destination_count,
        content: meta.content,
        timeline,
        usage: registry.usages(key),
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
