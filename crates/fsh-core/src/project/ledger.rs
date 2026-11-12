//! 项目写入口的落盘共用件：登记簿写回与机器提交消息。
//! 使用与撤回都经这里收尾——登记簿是项目侧用法的唯一记录，
//! 提交消息只用相对标识，项目写目录名、全局写 `global`。

use crate::commit::{Verb, project};
use crate::hub;
use crate::lock::WriteToken;
use crate::paths::HubPaths;

use super::{Anchor, ProjectError};

/// 登记簿落盘：读、改、写一次收口。任一步失败都原样上抛，
/// 盘面还原由写骨架的回滚清单承担——同一次操作不留半套状态
pub(super) fn commit_registry(
    paths: &HubPaths,
    token: &WriteToken,
    mutate: impl FnOnce(&mut hub::ProjectRegistry) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    let mut registry = hub::load_registry(paths)?;
    mutate(&mut registry)?;
    hub::save_registry(paths, &registry, token)?;
    Ok(())
}

/// 写命令机器提交消息：目标取调用方给的 agent 名单（同一目标内的 agent
/// 已由预检去重），对象取键与逐目标的项目名（全局目标写 `global`）。
/// 消息只允许相对标识——项目写目录名，不写绝对路径
pub(super) fn usage_message(
    verb: Verb,
    key: &str,
    agents: &[String],
    anchors: &[&Anchor],
) -> String {
    let mut names: Vec<String> = Vec::new();
    for anchor in anchors {
        let name = match anchor {
            Anchor::Project(dir) => dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project".to_string()),
            Anchor::Global => "global".to_string(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    project(verb, key, agents, &names)
}
