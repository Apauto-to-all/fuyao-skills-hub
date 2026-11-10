//! 端到端验收（dogfood）：仓库自带的伴生技能 fuyao-skills-hub 由 init 直接入库，
//! 真实子进程跑通「初始化 → 发版 → 全局使用/撤回 → 项目使用 → 归档 → 恢复 →
//! 彻底删除 → 再初始化还原」全链路。

mod common;

use common::{data_of, fsh, fsh_env, home_env};

#[test]
fn dogfood_lifecycle_end_to_end() {
    let install = tempfile::tempdir().unwrap();
    let hub = install.path().display().to_string();
    // ① 初始化 hub：三目录 + .gitignore 一次到位，伴生技能由 init 直接入库
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0, "init: {out}");
    let data = install.path();
    for dir in ["store", "export", "agents"] {
        assert!(data.join(dir).is_dir(), "init 应建出 {dir}/");
    }
    // 内置技能与普通技能同权：list 看得见，来源是仓库自己，首版 0.1.0
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let skills = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(skills.len(), 1, "init 后库里应有且只有内置技能: {skills:?}");
    // 键由程序给出（store 内相对路径），后续命令一律用它寻址
    let key = skills[0]["key"].as_str().expect("键应是字符串").to_string();
    assert_eq!(key, "builtin/fuyao-skills-hub");
    assert_eq!(skills[0]["latest_version"], serde_json::json!("0.1.0"));
    assert_eq!(
        skills[0]["source"],
        serde_json::json!("Apauto-to-all/fuyao-skills-hub")
    );
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", key.as_str()]);
    assert_eq!(code, 0, "show: {out}");
    assert_eq!(
        data_of(&out)["frontmatter"]["source"],
        serde_json::json!("Apauto-to-all/fuyao-skills-hub")
    );

    // ② 编辑面写下一个版本号再发版：号来自 frontmatter，tag 落在 git 里
    let work_md = data.join("store").join(&key).join("SKILL.md");
    let text = std::fs::read_to_string(&work_md)
        .unwrap()
        .replace("version: \"0.1.0\"", "version: \"0.2.0\"");
    std::fs::write(&work_md, text).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", key.as_str()]);
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

    // ③ 全局使用与撤回：命令形状与普通技能完全一致（落点在测试家目录）
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let (out, code) = fsh_env(
        &home_env(&home_s),
        &[
            "--json",
            "--hub",
            &hub,
            "use",
            key.as_str(),
            "--global",
            "--agent",
            "dsh",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");
    let d = data_of(&out);
    assert_eq!(d["targets"][0]["scope"], serde_json::json!("global"));
    assert_eq!(d["targets"][0]["destinations"].as_array().unwrap().len(), 1);
    assert!(
        home.path()
            .join(".dsh/skills/fuyao-skills-hub/SKILL.md")
            .is_file(),
        "全局落点应在测试家目录里"
    );
    let (out, code) = fsh_env(
        &home_env(&home_s),
        &["--json", "--hub", &hub, "unuse", key.as_str(), "--global"],
    );
    assert_eq!(code, 0, "unuse --global: {out}");
    assert!(!home.path().join(".dsh/skills/fuyao-skills-hub").exists());

    // ④ 项目使用：按固定版本铺一次，再整体撤回；被引用的技能不能归档
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let versioned = format!("{key}@0.2.0");
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        &versioned,
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
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", key.as_str()]);
    assert_eq!(code, 1, "被引用时 archive 应拒绝: {out}");
    // 撤回：落点与登记一起清
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "unuse",
        key.as_str(),
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

    // ⑤ 归档：编辑面消失，版本历史仍在 list 里
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", key.as_str()]);
    assert_eq!(code, 0, "archive: {out}");
    assert!(!work_md.exists(), "archive 应删掉工作副本");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let skills = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["archived"], serde_json::json!(true));
    assert_eq!(skills[0]["latest_version"], serde_json::json!("0.2.0"));

    // ⑥ 恢复：编辑面按最新 tag 复活，版本链原样（仍是 0.2.0，没有新版本）
    let (out, code) = fsh(&["--json", "--hub", &hub, "restore", key.as_str()]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(work_md.is_file(), "restore 应复活工作副本");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let skills = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(skills[0]["archived"], serde_json::json!(false));
    assert_eq!(skills[0]["latest_version"], serde_json::json!("0.2.0"));

    // ⑦ 彻底删除：连 tag 一起清，list 里不再有它
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", key.as_str()]);
    assert_eq!(code, 0, "remove: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    assert!(
        data_of(&out)["skills"].as_array().unwrap().is_empty(),
        "remove 后不应再有该 skill"
    );

    // ⑧ 再 init：内置技能按程序内嵌内容还原（内容与首版号都回到 0.1.0）
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0, "init again: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let skills = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(skills.len(), 1, "init 应把内置技能还原: {skills:?}");
    assert_eq!(skills[0]["key"], serde_json::json!(key));
    assert_eq!(skills[0]["archived"], serde_json::json!(false));
    assert_eq!(skills[0]["latest_version"], serde_json::json!("0.1.0"));
    assert!(
        std::fs::read_to_string(&work_md)
            .unwrap()
            .contains("version: \"0.1.0\""),
        "还原的应是程序内嵌的那份"
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
