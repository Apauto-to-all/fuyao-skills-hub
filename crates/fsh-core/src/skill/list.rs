//! 列表：浏览视图——一行一个技能，键、description 与使用清单。
//! 使用清单是登记簿的原样投影（作用域、目标、链接方式与 agent 名单），
//! 未使用的技能为空。归档过滤旗标切换到归档面；缺省只列未归档。
//! 前缀收窄到某一套；与位置过滤词取交集。一次性全量输出，无分页。

use std::fs;
use std::path::Path;

use serde::Serialize;

use super::SkillError;
use super::frontmatter::parse;
use super::walk_face;
use crate::hub::SkillUsage;
use crate::paths::HubPaths;
use crate::select;

/// 列表的一行：一个技能的浏览摘要
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillRow {
    /// skill 唯一键
    pub key: String,
    /// 一句话描述（frontmatter 解析失败时为空串，不中断整页）
    pub description: String,
    /// 使用清单：登记簿里该技能的全部登记记录，逐记录原样带出；未使用为空
    pub uses: Vec<SkillUsage>,
}

/// 列出技能：`archived = false` 列 store 面的活跃技能，`true` 列 archive 面
/// 的归档技能；`filter` 按键 / description 子串过滤（忽略大小写），
/// `prefix` 把面收窄到某一套（缺省整面）。键按字典序返回
pub fn list(
    hub_root: &Path,
    filter: Option<&str>,
    archived: bool,
    prefix: Option<&str>,
) -> Result<Vec<SkillRow>, SkillError> {
    let paths = HubPaths::from_root(hub_root);
    // 装配判定与稳定码由 open_hub 单点持有
    crate::git::open_hub(hub_root)?;
    let registry = crate::hub::load_registry(&paths)?;
    let face = if archived {
        paths.archive()
    } else {
        paths.store()
    };
    let prefix = select::normalize(prefix);
    let mut keys: Vec<String> = walk_face(&face)
        .filter(|skill| skill.has_skill_md)
        .filter(|skill| select::in_set(&skill.key, prefix))
        .map(|skill| skill.key)
        .collect();
    keys.sort();
    let needle = filter.map(str::to_lowercase);
    let rows = keys
        .into_iter()
        .filter_map(|key| {
            let dir = face.join(&key);
            let description = read_summary(&dir, &key, archived);
            // 子串过滤：键与 description 都算命中（忽略大小写）
            if let Some(needle) = &needle {
                let hit = key.to_lowercase().contains(needle)
                    || description.to_lowercase().contains(needle);
                if !hit {
                    return None;
                }
            }
            Some(SkillRow {
                uses: registry.usages(&key),
                key,
                description,
            })
        })
        .collect();
    Ok(rows)
}

/// 读一个技能的摘要：description；读不到或解析失败都降级为空串。
/// 报错标签按实际所在面取名（store / archive）
fn read_summary(dir: &Path, key: &str, archived: bool) -> String {
    let subpath = if archived {
        crate::naming::archive_subpath(key)
    } else {
        crate::naming::store_subpath(key)
    };
    let label = format!("{subpath}/SKILL.md");
    let Ok(text) = fs::read_to_string(dir.join("SKILL.md")) else {
        return String::new();
    };
    match parse(&text, &label) {
        Ok(fm) => fm.description,
        Err(e) => {
            tracing::warn!(key = %key, cause = %e, "跳过无法解析的 skill");
            String::new()
        }
    }
}
