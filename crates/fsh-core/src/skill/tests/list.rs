//! 列表测试：行内容、过滤词与归档旗标。

use super::*;

#[test]
fn list_rows_carry_key_description_source_and_usage() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", None, "测试驱动开发"),
        None,
        false,
    )
    .unwrap();
    let proj = tempfile::tempdir().unwrap();
    register_usage(&paths, proj.path(), "tdd");

    let rows = list(&root(&install), None, false).unwrap();
    // init 落地的内置技能与刚入库的技能都在
    let tdd = rows.iter().find(|r| r.key == "tdd").unwrap();
    assert_eq!(tdd.description, "测试驱动开发");
    assert_eq!(tdd.source, None);
    // 使用清单是登记记录的原样投影：一条记录一行，带目标、链接方式与 agent 名单
    assert_eq!(tdd.uses.len(), 1, "登记簿一条记录出一次");
    assert_eq!(tdd.uses[0].scope, crate::hub::Scope::Project);
    assert!(tdd.uses[0].project.is_some(), "项目记录带项目路径");
    assert_eq!(tdd.uses[0].link, crate::link::LinkKind::Junction);
    assert_eq!(tdd.uses[0].agents, vec!["universal".to_string()]);
    let builtin = rows.iter().find(|r| r.key.starts_with("builtin/")).unwrap();
    assert!(builtin.source.is_some(), "内置技能带 source");
    assert!(builtin.uses.is_empty(), "没登记过的技能使用清单为空");
}

/// 过滤词按键 / description 子串命中（忽略大小写），不命中的一行不出
#[test]
fn list_filters_by_key_or_description_substring() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "tdd", None, "测试驱动开发"),
        None,
        false,
    )
    .unwrap();

    let rows = list(&root(&install), Some("tdd"), false).unwrap();
    assert!(
        rows.iter()
            .all(|r| r.key.contains("tdd") || r.description.contains("tdd"))
    );
    assert!(rows.iter().any(|r| r.key == "tdd"), "键命中: {rows:?}");
    let rows = list(&root(&install), Some("TDD"), false).unwrap();
    assert!(rows.iter().any(|r| r.key == "tdd"), "过滤忽略大小写");
    assert!(
        list(&root(&install), Some("绝对不存在的词"), false)
            .unwrap()
            .is_empty()
    );
}

/// 缺省只列未归档；旗标切换到归档面
#[test]
fn list_archived_flag_switches_face() {
    let (install, _paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "gone", None, "归档过滤"),
        None,
        false,
    )
    .unwrap();
    archive(&root(&install), "gone", false).unwrap();

    let rows = list(&root(&install), None, false).unwrap();
    assert!(!rows.iter().any(|r| r.key == "gone"), "缺省不列归档技能");
    let rows = list(&root(&install), None, true).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "gone");
}
