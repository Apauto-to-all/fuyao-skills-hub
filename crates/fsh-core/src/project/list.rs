//! 项目视图：登记簿与落点解析的只读投影，供界面按项目看占用情况。
//! 落点只查 exists；链接类型归 doctor，copy 内容比对归 unuse --force。

use std::path::Path;

use serde::Serialize;

use super::{ProjectError, dest};
use crate::agents;
use crate::hub;
use crate::link::LinkKind;
use crate::paths::HubPaths;

/// 一处落点的只读状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DestinationView {
    /// 落点绝对路径
    pub path: String,
    /// 解析到该落点的 agent 键
    pub agents: Vec<String>,
    /// 此刻是否存在于磁盘
    pub exists: bool,
}

/// 项目里一条技能的登记视图
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectSkillView {
    /// skill 唯一键
    pub key: String,
    /// 登记的 agent 键
    pub agents: Vec<String>,
    /// 登记的链接方式
    pub link: LinkKind,
    /// 链接方式的人读名：界面直接显示，文案只由 `LinkKind` 的 Display 提供
    pub link_label: String,
    /// 解析出的落点与存在状态
    pub destinations: Vec<DestinationView>,
}

/// 一个项目的全量视图
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectView {
    /// 项目根绝对路径
    pub path: String,
    /// 最后一次被 fsh 操作的时间（Unix 秒）
    pub last_used: u64,
    /// 该项目登记的全部技能
    pub skills: Vec<ProjectSkillView>,
}

/// 列出全部已登记项目及其技能、落点状态。
/// 单条技能的 agent 配置读不出来时只降级该条（落点留空），不让整页报错
pub fn list(hub_root: &Path) -> Result<Vec<ProjectView>, ProjectError> {
    let paths = HubPaths::from_root(hub_root);
    let registry = hub::load_registry(&paths)?;
    // agent 目录整页只扫一次：逐 agent / 逐技能重复扫描会把 INFO 刷满
    let known = agents::list(&paths)?;
    let mut projects = Vec::with_capacity(registry.projects.len());
    // 表键就是项目身份（规范化绝对路径），直接用作落点解析的基准目录
    for (path, record) in registry.projects {
        let project = Path::new(&path);
        let mut skills = Vec::with_capacity(record.skills.len());
        for skill in record.skills {
            let destinations =
                match dest::resolve_project(&known, project, &skill.agents, &skill.key) {
                    Ok(found) => found
                        .into_iter()
                        .map(|dest| DestinationView {
                            exists: Path::new(&dest.path).exists(),
                            path: dest.path,
                            agents: dest.agents,
                        })
                        .collect(),
                    Err(error) => {
                        tracing::warn!(
                            project = %path,
                            key = %skill.key,
                            cause = %error,
                            "落点解析降级"
                        );
                        Vec::new()
                    }
                };
            skills.push(ProjectSkillView {
                key: skill.key,
                agents: skill.agents,
                link_label: skill.link.to_string(),
                link: skill.link,
                destinations,
            });
        }
        projects.push(ProjectView {
            // 界面形态：去掉 Windows 扩展前缀；文件操作仍用记录里的原样路径
            path: crate::identity::display_path(Path::new(&path)),
            last_used: record.last_used,
            skills,
        });
    }
    Ok(projects)
}
