//! 端到端验收（dogfood）：仓库自带的伴生 skill manage-skills 当第一个
//! 入库对象，真实子进程跑通「初始化 → 入库 → 发版 → 归档 → 恢复 → 彻底删除」全链路。

use std::path::Path;

mod common;

use common::{data_of, fsh};

#[test]
fn dogfood_lifecycle_end_to_end() {
    let install = tempfile::tempdir().unwrap();
    let hub = install.path().join("data").display().to_string();
    // 伴生 skill 源 = 本仓库 skills/manage-skills（自己吃自己的狗粮）
    let skill_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/manage-skills");
    let src = skill_src.display().to_string();

    // ① 初始化 hub：三目录 + .gitignore 一次到位
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0, "init: {out}");
    let data = install.path().join("data");
    for dir in ["store", "export", "agents"] {
        assert!(data.join(dir).is_dir(), "init 应建出 {dir}/");
    }

    // ② 入库伴生 skill 本体：首版 0.1.0 一步到位
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src]);
    assert_eq!(code, 0, "add: {out}");
    let d = data_of(&out);
    assert_eq!(d["key"], serde_json::json!("manage-skills"));
    assert_eq!(d["version"], serde_json::json!("0.1.0"));

    // ③ 编辑面写下一个版本号再发版：号来自 frontmatter，tag 落在 git 里
    let work_md = data.join("store").join("manage-skills").join("SKILL.md");
    let text = std::fs::read_to_string(&work_md)
        .unwrap()
        .replace("version: \"0.1.0\"", "version: \"0.2.0\"");
    std::fs::write(&work_md, text).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "manage-skills"]);
    assert_eq!(code, 0, "release: {out}");
    assert_eq!(data_of(&out)["version"], serde_json::json!("0.2.0"));
    // 发版不产物化目录：export/ 里只允许出现锁文件
    let exported: Vec<_> = std::fs::read_dir(data.join("export"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n != ".lock")
        .collect();
    assert!(
        exported.is_empty(),
        "release 不应生成导出目录: {exported:?}"
    );

    // ④ 归档：编辑面消失，版本历史仍在 list 里
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "manage-skills"]);
    assert_eq!(code, 0, "archive: {out}");
    assert!(!work_md.exists(), "archive 应删掉工作副本");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let skills = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["archived"], serde_json::json!(true));
    assert_eq!(skills[0]["latest_version"], serde_json::json!("0.2.0"));

    // ⑤ 恢复：编辑面按最新 tag 复活，版本链原样（仍是 0.2.0，没有新版本）
    let (out, code) = fsh(&["--json", "--hub", &hub, "restore", "manage-skills"]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(work_md.is_file(), "restore 应复活工作副本");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let skills = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(skills[0]["archived"], serde_json::json!(false));
    assert_eq!(skills[0]["latest_version"], serde_json::json!("0.2.0"));

    // ⑥ 项目使用：先跟随最新铺一次，撤回后按固定版本重铺（升级 / 回滚路径），
    //    再整体撤回，最后验证被引用的技能不能归档
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "manage-skills@0.2.0",
        "--agent",
        "universal,dsh",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "use: {out}");
    let d = data_of(&out);
    assert_eq!(d["version"], serde_json::json!("0.2.0"));
    assert_eq!(d["destinations"].as_array().unwrap().len(), 2);
    assert!(
        project
            .path()
            .join(".agents/skills/manage-skills/SKILL.md")
            .is_file()
    );
    assert!(
        project
            .path()
            .join(".dsh/skills/manage-skills/SKILL.md")
            .is_file()
    );
    // 归档不得动被项目引用的技能
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "manage-skills"]);
    assert_eq!(code, 1, "被引用时 archive 应拒绝: {out}");
    // 撤回：落点与登记一起清
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "unuse",
        "manage-skills",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "unuse: {out}");
    assert!(!project.path().join(".agents/skills/manage-skills").exists());
    assert!(!project.path().join(".dsh/skills/manage-skills").exists());

    // ⑦ 彻底删除：连 tag 一起清，list 里不再有它
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "manage-skills"]);
    assert_eq!(code, 0, "remove: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    assert!(
        data_of(&out)["skills"].as_array().unwrap().is_empty(),
        "remove 后不应再有该 skill"
    );
}
