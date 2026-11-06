//! 归档测试：工作副本撤走而版本与导出目录保留。

use std::fs;

use super::*;

#[test]
fn archive_removes_workdir_but_keeps_versions_and_export() {
    let (install, paths) = setup();
    let src_dir = tempfile::tempdir().unwrap();
    add(
        &root(&install),
        &make_skill(src_dir.path(), "arch", None, "归档供版"),
        None,
        false,
    )
    .unwrap();
    // 先造一个导出目录，归档要连它一起收
    let exported = paths.exported("arch", "0.1.0");
    fs::create_dir_all(&exported).unwrap();
    fs::write(exported.join("SKILL.md"), "# arch\n").unwrap();

    archive(&root(&install), "arch", false).unwrap();
    // 编辑面与导出目录删除后，该键只剩下 tag 与 git 历史
    assert!(!paths.skill_workdir("arch").exists());
    assert!(!paths.export().join("arch").exists());
    assert!(!paths.root().join("meta").exists());
    // 版本链全在、历史提交可查
    let repo = git2::Repository::open(paths.root()).unwrap();
    assert_eq!(git::tag_list(&repo, "arch").unwrap(), vec!["0.1.0"]);
    assert!(!git::is_dirty(&repo).unwrap());
}
