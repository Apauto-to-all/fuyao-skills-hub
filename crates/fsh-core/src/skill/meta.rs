//! SKILL.md 元数据：统一活跃态（工作副本）与归档态（最新 tag 树）的取文与解析。
//! 查询面把读不到、解析不出都当降级（返回 None），不因单条脏文件中断整页。

use std::fs;

use super::frontmatter::parse;
use super::{Frontmatter, SkillError, skill_md_from_tag, workdir_exists};
use crate::paths::HubPaths;

/// 读某键的 SKILL.md 原文：活跃态读工作副本，归档态读最新 tag 树。
/// 工作副本读不到、或归档态没有版本时返回 None——查询面不因脏文件报错
pub(super) fn skill_md_text(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    versions: &[String],
) -> Result<Option<String>, SkillError> {
    if workdir_exists(paths, key) {
        let file = paths.skill_workdir(key).join("SKILL.md");
        match fs::read_to_string(&file) {
            Ok(text) => Ok(Some(text)),
            // 工作副本在场却读不到 SKILL.md 是脏文件：查询面照旧返回空值，但留痕供排查
            Err(e) => {
                tracing::warn!(
                    key = %key,
                    path = %file.display(),
                    cause = %e,
                    "读取 skill 元数据失败"
                );
                Ok(None)
            }
        }
    } else {
        let Some(latest) = versions.first() else {
            return Ok(None);
        };
        skill_md_from_tag(repo, &crate::naming::version_tag(key, latest), key)
    }
}

/// 原文与 frontmatter 一次取全：list / show 共用，一段内容只读一次
pub(super) fn read_meta(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    versions: &[String],
) -> Result<(Option<String>, Option<Frontmatter>), SkillError> {
    let content = skill_md_text(repo, paths, key, versions)?;
    let frontmatter = content.as_deref().and_then(|text| parse_meta(text, key));
    Ok((content, frontmatter))
}

/// 解析失败同样是降级：查询面不因单条坏 frontmatter 中断，但必须能查出是哪个键
fn parse_meta(text: &str, key: &str) -> Option<Frontmatter> {
    let label = format!("{}/SKILL.md", crate::naming::store_subpath(key));
    match parse(text, &label) {
        Ok(fm) => Some(fm),
        Err(e) => {
            tracing::warn!(key = %key, cause = %e, "跳过无法解析的 skill");
            None
        }
    }
}
