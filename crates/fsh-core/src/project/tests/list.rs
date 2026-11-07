//! 项目视图测试：落点存在与缺失两态，以及 agent 使用记录的多项目聚合。

use std::fs;

use super::*;
use crate::agents;

#[test]
fn list_reports_each_destination_and_its_missing_state() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    use_skill(
        paths.root(),
        "demo",
        None,
        &two_agents(),
        UseTargets::project(project.as_path()),
        false,
    )
    .unwrap();

    let projects = list(paths.root()).unwrap();
    assert_eq!(projects.len(), 1);
    let skill = &projects[0].skills[0];
    assert_eq!(skill.key, "demo");
    // 人读名必须由 Display 产出：界面直接显示它，自己不再带词表
    assert_eq!(skill.link_label, skill.link.to_string());
    assert_eq!(skill.destinations.len(), 2, "两个 agent 落在不同目录");
    assert!(skill.destinations.iter().all(|d| d.exists));

    // 手删一个落点：视图应把它标成缺失，而不是让整条记录消失
    fs::remove_dir(skill.destinations[0].path.as_str()).unwrap();
    let after = list(paths.root()).unwrap();
    let states: Vec<bool> = after[0].skills[0]
        .destinations
        .iter()
        .map(|d| d.exists)
        .collect();
    assert_eq!(states.iter().filter(|exists| **exists).count(), 1);
    assert_eq!(states.iter().filter(|exists| !**exists).count(), 1);
}

#[test]
fn agent_usages_aggregate_across_projects() {
    let (_install, paths) = setup_with_skill(None);
    let first = temp_project();
    let second = temp_project();
    use_skill(
        paths.root(),
        "demo",
        None,
        &["universal".to_string()],
        UseTargets::project(first.as_path()),
        false,
    )
    .unwrap();
    use_skill(
        paths.root(),
        "demo",
        None,
        &["dsh".to_string()],
        UseTargets::project(second.as_path()),
        false,
    )
    .unwrap();

    let rows = agents::list(&paths).unwrap();
    let usage_of = |key: &str| {
        rows.iter()
            .find(|agent| agent.key == key)
            .map(|agent| agent.usages.clone())
            .unwrap()
    };
    let universal = usage_of("universal");
    let dsh = usage_of("dsh");
    assert_eq!(universal.len(), 1);
    assert_eq!(dsh.len(), 1);
    assert_eq!(universal[0].key, "demo");
    assert_ne!(universal[0].project, dsh[0].project, "两个项目各记一条");
}

#[test]
fn unknown_agent_degrades_one_skill_instead_of_failing_the_page() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    // 直接写登记簿：模拟用户删掉/改名了某个 agent 的配置
    let project_path = project.display().to_string();
    let text = format!(
        "[projects.'{project_path}']\nlast_used = 1\n\n\
         [[projects.'{project_path}'.skills]]\nkey = \"demo\"\nagents = [\"ghost\"]\nlink = \"junction\"\n"
    );
    fs::write(paths.projects_toml(), text).unwrap();

    let projects = list(paths.root()).unwrap();
    assert_eq!(projects.len(), 1, "坏记录不让整页报错");
    assert!(projects[0].skills[0].destinations.is_empty());
}
