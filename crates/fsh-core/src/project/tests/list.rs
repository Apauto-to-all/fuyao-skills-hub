//! 项目视图测试：落点存在与缺失两态。

use std::fs;

use super::*;

#[test]
fn list_reports_each_destination_and_its_missing_state() {
    let (_install, paths) = setup_with_skill(None);
    let project = temp_project();
    use_skill(
        paths.root(),
        "demo",
        None,
        &two_agents(),
        Targets::new(false, vec![project.as_path().into()]),
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
