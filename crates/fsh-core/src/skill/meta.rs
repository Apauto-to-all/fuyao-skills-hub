//! SKILL.md 元数据：按技能状态模块的三态取文——活跃读工作副本、归档读最新
//! tag 树——并统一解析。查询面把读不到、解析不出都当降级（元数据为空），
//! 不因单条脏文件中断整页；解析失败的原因随结果一起带出，
//! 单个技能的详情页据此点名字段。

use std::fs;

use super::frontmatter::parse;
use super::{Frontmatter, SkillError, SkillState, SkillStatus};
use crate::git;
use crate::paths::HubPaths;

/// 一次取全的元数据：原文、解析结果与解析失败原因
pub(super) struct Meta {
    /// SKILL.md 原文（读不到为 None）
    pub content: Option<String>,
    /// frontmatter 解析结果（读不到或解析不出为 None）
    pub frontmatter: Option<Frontmatter>,
    /// 解析失败原因（点名文件与字段；解析成功或读不到文件时为 None）
    pub parse_error: Option<String>,
}

/// 读某键的 SKILL.md 原文：活跃态读工作副本，归档态读最新 tag 树。
/// 工作副本读不到、或归档态没有版本时返回 None——查询面不因脏文件报错
pub(super) fn skill_md_text(
    repo: &git::Repository,
    paths: &HubPaths,
    status: &SkillStatus,
) -> Result<Option<String>, SkillError> {
    if status.state() == SkillState::Active {
        let file = paths.skill_workdir(status.key()).join("SKILL.md");
        match fs::read_to_string(&file) {
            Ok(text) => Ok(Some(text)),
            // 工作副本在场却读不到 SKILL.md 是脏文件：查询面照旧返回空值，但留痕供排查
            Err(e) => {
                tracing::warn!(
                    key = %status.key(),
                    path = %file.display(),
                    cause = %e,
                    "读取 skill 元数据失败"
                );
                Ok(None)
            }
        }
    } else {
        let Some(latest) = status.latest() else {
            return Ok(None);
        };
        let tag = crate::naming::version_tag(status.key(), latest);
        let subpath = format!("{}/SKILL.md", crate::naming::store_subpath(status.key()));
        Ok(git::read_blob_at(repo, &tag, &subpath)?)
    }
}

/// 原文与 frontmatter 一次取全：list / show 共用，一段内容只读一次
pub(super) fn read_meta(
    repo: &git::Repository,
    paths: &HubPaths,
    status: &SkillStatus,
) -> Result<Meta, SkillError> {
    let content = skill_md_text(repo, paths, status)?;
    let (frontmatter, parse_error) = match content.as_deref() {
        Some(text) => parse_meta(text, status.key()),
        None => (None, None),
    };
    Ok(Meta {
        content,
        frontmatter,
        parse_error,
    })
}

/// 解析失败是降级：查询面不因单条坏 frontmatter 中断，但失败原因要带出去，
/// 且必须能查出是哪个键
fn parse_meta(text: &str, key: &str) -> (Option<Frontmatter>, Option<String>) {
    let label = format!("{}/SKILL.md", crate::naming::store_subpath(key));
    match parse(text, &label) {
        Ok(fm) => (Some(fm), None),
        Err(e) => {
            tracing::warn!(key = %key, cause = %e, "跳过无法解析的 skill");
            (None, Some(e.to_string()))
        }
    }
}
