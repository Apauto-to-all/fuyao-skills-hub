//! 端到端验收（dogfood）：仓库自带的伴生技能 fuyao-skills-hub 由 init 直接入库，
//! 真实子进程跑通「初始化 → 发版 → 全局使用/撤回 → 项目使用 → 归档 → 恢复 →
//! 彻底删除 → 再初始化还原」全链路。

mod common;

use common::{data_of, error_of, fsh, fsh_env, home_env};

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
    // 内置技能与普通技能同权：show 看得见，来源是仓库自己，首版 0.1.0
    // 键由程序给出（store 内相对路径），后续命令一律用它寻址
    let key = "builtin/fuyao-skills-hub";
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", key]);
    assert_eq!(code, 0, "show: {out}");
    let detail = data_of(&out);
    assert_eq!(detail["latest_version"], serde_json::json!("0.1.0"));
    assert_eq!(
        detail["frontmatter"]["source"],
        serde_json::json!("Apauto-to-all/fuyao-skills-hub")
    );

    // ② 编辑面写下一个版本号再发版：号来自 frontmatter，tag 落在 git 里
    let work_md = data.join("store").join(key).join("SKILL.md");
    let text = std::fs::read_to_string(&work_md)
        .unwrap()
        .replace("version: \"0.1.0\"", "version: \"0.2.0\"");
    std::fs::write(&work_md, text).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", key]);
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
            "--json", "--hub", &hub, "use", key, "--global", "--agent", "dsh",
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
        &["--json", "--hub", &hub, "unuse", key, "--global"],
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
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", key]);
    assert_eq!(code, 1, "被引用时 archive 应拒绝: {out}");
    // 撤回：落点与登记一起清
    let (out, code) = fsh(&["--json", "--hub", &hub, "unuse", key, "--project", &proj]);
    assert_eq!(code, 0, "unuse: {out}");
    assert!(
        !project
            .path()
            .join(".agents/skills/fuyao-skills-hub")
            .exists()
    );
    assert!(!project.path().join(".dsh/skills/fuyao-skills-hub").exists());

    // ⑤ 归档：编辑面消失，版本历史仍可查（show 仍可看）
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", key]);
    assert_eq!(code, 0, "archive: {out}");
    assert!(!work_md.exists(), "archive 应删掉工作副本");
    let (out, _) = fsh(&["--json", "--hub", &hub, "show", key]);
    assert_eq!(data_of(&out)["archived"], serde_json::json!(true));
    assert_eq!(data_of(&out)["latest_version"], serde_json::json!("0.2.0"));

    // ⑥ 恢复：编辑面按最新 tag 复活，版本链原样（仍是 0.2.0，没有新版本）
    let (out, code) = fsh(&["--json", "--hub", &hub, "restore", key]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(work_md.is_file(), "restore 应复活工作副本");
    let (out, _) = fsh(&["--json", "--hub", &hub, "show", key]);
    assert_eq!(data_of(&out)["archived"], serde_json::json!(false));
    assert_eq!(data_of(&out)["latest_version"], serde_json::json!("0.2.0"));

    // ⑦ 彻底删除：连 tag 一起清，show 报 skill_not_found
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", key]);
    assert_eq!(code, 0, "remove: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", key]);
    assert_eq!(code, 1, "remove 后 show 应报不存在: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("skill_not_found"));

    // ⑧ 再 init：内置技能按程序内嵌内容还原（内容与首版号都回到 0.1.0）
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0, "init again: {out}");
    let (out, _) = fsh(&["--json", "--hub", &hub, "show", key]);
    assert_eq!(data_of(&out)["archived"], serde_json::json!(false));
    assert_eq!(data_of(&out)["latest_version"], serde_json::json!("0.1.0"));
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
