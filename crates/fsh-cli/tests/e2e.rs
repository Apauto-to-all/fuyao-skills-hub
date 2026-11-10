//! 端到端验收（dogfood）：仓库自带的伴生 skill fuyao-skills-hub 当第一个
//! 入库对象，真实子进程跑通「初始化 → 入库 → 发版 → 归档 → 恢复 → 彻底删除」全链路。

use std::path::Path;

mod common;

use common::{data_of, fsh};

#[test]
fn dogfood_lifecycle_end_to_end() {
    let install = tempfile::tempdir().unwrap();
    let hub = install.path().display().to_string();
    // 伴生 skill 源 = 本仓库 skills/fuyao-skills-hub（自己吃自己的狗粮）
    let skill_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/fuyao-skills-hub");
    let src = skill_src.display().to_string();

    // ① 初始化 hub：三目录 + .gitignore 一次到位
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0, "init: {out}");
    let data = install.path();
    for dir in ["store", "export", "agents"] {
        assert!(data.join(dir).is_dir(), "init 应建出 {dir}/");
    }

    // ② 入库伴生 skill 本体：首版 0.1.0 一步到位
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src]);
    assert_eq!(code, 0, "add: {out}");
    let d = data_of(&out);
    assert_eq!(d["key"], serde_json::json!("fuyao-skills-hub"));
    assert_eq!(d["version"], serde_json::json!("0.1.0"));

    // ③ 编辑面写下一个版本号再发版：号来自 frontmatter，tag 落在 git 里
    let work_md = data.join("store").join("fuyao-skills-hub").join("SKILL.md");
    let text = std::fs::read_to_string(&work_md)
        .unwrap()
        .replace("version: \"0.1.0\"", "version: \"0.2.0\"");
    std::fs::write(&work_md, text).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "fuyao-skills-hub"]);
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
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "fuyao-skills-hub"]);
    assert_eq!(code, 0, "archive: {out}");
    assert!(!work_md.exists(), "archive 应删掉工作副本");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let skills = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["archived"], serde_json::json!(true));
    assert_eq!(skills[0]["latest_version"], serde_json::json!("0.2.0"));

    // ⑤ 恢复：编辑面按最新 tag 复活，版本链原样（仍是 0.2.0，没有新版本）
    let (out, code) = fsh(&["--json", "--hub", &hub, "restore", "fuyao-skills-hub"]);
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
        "fuyao-skills-hub@0.2.0",
        "--agent",
        "universal,dsh",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "use: {out}");
    let d = data_of(&out);
    assert_eq!(d["version"], serde_json::json!("0.2.0"));
    assert_eq!(d["targets"][0]["destinations"].as_array().unwrap().len(), 2);
    assert!(
        project
            .path()
            .join(".agents/skills/fuyao-skills-hub/SKILL.md")
            .is_file()
    );
    assert!(
        project
            .path()
            .join(".dsh/skills/fuyao-skills-hub/SKILL.md")
            .is_file()
    );
    // 归档不得动被项目引用的技能
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "fuyao-skills-hub"]);
    assert_eq!(code, 1, "被引用时 archive 应拒绝: {out}");
    // 撤回：落点与登记一起清
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "unuse",
        "fuyao-skills-hub",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "unuse: {out}");
    assert!(
        !project
            .path()
            .join(".agents/skills/fuyao-skills-hub")
            .exists()
    );
    assert!(!project.path().join(".dsh/skills/fuyao-skills-hub").exists());

    // ⑦ 彻底删除：连 tag 一起清，list 里不再有它
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "fuyao-skills-hub"]);
    assert_eq!(code, 0, "remove: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    assert!(
        data_of(&out)["skills"].as_array().unwrap().is_empty(),
        "remove 后不应再有该 skill"
    );
}

/// 首次 init 时 hub 目录还不存在，日志仍要落盘：日志目录由日志模块自建，
/// 不把「目录已存在」当前置条件，否则最该留痕的命令反而没有日志。
#[test]
fn init_writes_log_before_hub_exists() {
    let install = tempfile::tempdir().unwrap();
    let data = install.path().join("hub");
    let hub = data.display().to_string();
    assert!(!data.exists(), "前置：hub 目录尚不存在");

    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0, "init: {out}");

    let logs: Vec<_> = std::fs::read_dir(data.join("logs"))
        .expect("init 应建出 logs/")
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(logs.len(), 1, "当天应只有一个日志文件: {logs:?}");
    let text = std::fs::read_to_string(&logs[0]).unwrap();
    assert!(
        text.contains("Fuyao Skills Hub 初始化完成"),
        "日志应留下初始化事件: {text}"
    );
    assert!(text.contains("cmd=init"), "日志应留下命令完成: {text}");
}
