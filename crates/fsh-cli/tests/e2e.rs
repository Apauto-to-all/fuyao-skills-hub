//! 端到端验收（dogfood）：仓库自带的伴生技能 fuyao-skills-hub 由 init 直接入库，
//! 真实子进程跑通「初始化 → 全局使用/撤回 → 项目使用 → 归档 → 恢复 →
//! 彻底删除 → 再初始化还原」全链路。

mod common;

use common::{Run, data_of, home_env, json, json_env};

#[test]
fn dogfood_lifecycle_end_to_end() {
    let install = tempfile::tempdir().unwrap();
    let hub = install.path().display().to_string();
    // ① 初始化 hub：三目录 + .gitignore 一次到位，伴生技能由 init 直接入库
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["init"]);
    assert_eq!(code, 0, "init: {out}");
    let data = install.path();
    for dir in ["store", "archive", "agents"] {
        assert!(data.join(dir).is_dir(), "init 应建出 {dir}/");
    }
    // 内置技能与普通技能同权：list 看得见
    let key = "builtin/fuyao-skills-hub";
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list"]);
    assert_eq!(code, 0, "list: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    rows.iter()
        .find(|r| r["key"] == key)
        .expect("内置技能在列表");

    // ② 全局使用与撤回：命令形状与普通技能完全一致（落点在测试家目录）
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let Run {
        stdout: out, code, ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &[
            "use",
            "--prefix",
            "builtin",
            "--skill",
            "fuyao-skills-hub",
            "--global",
            "--agent",
            "dsh",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["builtin/fuyao-skills-hub"])
    );
    assert!(d["skipped"].as_array().unwrap().is_empty(), "{d}");
    assert!(
        home.path()
            .join(".dsh/skills/fuyao-skills-hub/SKILL.md")
            .is_file(),
        "全局落点应在测试家目录里"
    );
    let Run {
        stdout: out, code, ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &["unuse", key, "--agent", "dsh", "--global"],
    );
    assert_eq!(code, 0, "unuse --global: {out}");
    assert!(!home.path().join(".dsh/skills/fuyao-skills-hub").exists());

    // ③ 项目使用：落点直连工作副本，再整体撤回；被引用的技能不能归档
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "use",
            "--prefix",
            "builtin",
            "--skill",
            "fuyao-skills-hub",
            "--agent",
            "universal,dsh",
            "--project",
            &proj,
        ],
    );
    assert_eq!(code, 0, "use: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["builtin/fuyao-skills-hub"])
    );
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
    // 归档不得动被项目引用的技能：那条进 skipped，一条都没做成因此退出码 1
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "archive",
            "--prefix",
            "builtin",
            "--skill",
            "fuyao-skills-hub",
        ],
    );
    assert_eq!(code, 1, "一条都没做成: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!([]), "{out}");
    assert_eq!(
        d["skipped"][0]["code"],
        serde_json::json!("referenced"),
        "{out}"
    );
    assert!(
        data.join("store").join(key).join("SKILL.md").is_file(),
        "跳过时工作副本不动"
    );
    // 撤回：落点与登记一起清
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["unuse", key, "--agent", "universal,dsh", "--project", &proj],
    );
    assert_eq!(code, 0, "unuse: {out}");
    assert!(
        !project
            .path()
            .join(".agents/skills/fuyao-skills-hub")
            .exists()
    );
    assert!(!project.path().join(".dsh/skills/fuyao-skills-hub").exists());

    // ④ 归档：工作副本整目录移入 archive，只在归档旗标下列出
    let work_md = data.join("store").join(key).join("SKILL.md");
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "archive",
            "--prefix",
            "builtin",
            "--skill",
            "fuyao-skills-hub",
        ],
    );
    assert_eq!(code, 0, "archive: {out}");
    assert!(!work_md.exists(), "archive 应移走工作副本");
    assert!(
        data.join("archive").join(key).join("SKILL.md").is_file(),
        "归档副本应落在 archive/"
    );
    let Run { stdout: out, .. } = json(&hub, &["list", "--archived"]);
    let archived = data_of(&out)["skills"].as_array().unwrap().clone();
    assert!(
        archived.iter().any(|r| r["key"] == key),
        "归档面可见: {archived:?}"
    );

    // ⑤ 恢复：整目录移回 store
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", key]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(work_md.is_file(), "restore 应移回工作副本");
    assert!(!data.join("archive").join(key).exists());

    // ⑥ 彻底删除：store 与 archive 都清掉，列表与归档面都没有它
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", key]);
    assert_eq!(code, 0, "remove: {out}");
    assert!(!work_md.exists());
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list"]);
    assert_eq!(code, 0);
    assert!(
        !data_of(&out)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == key)
    );

    // ⑦ 再 init：内置技能按程序内嵌内容还原
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["init"]);
    assert_eq!(code, 0, "init again: {out}");
    let Run { stdout: out, .. } = json(&hub, &["list"]);
    assert!(
        data_of(&out)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == key),
        "init 应还原内置技能"
    );
    assert!(
        std::fs::read_to_string(&work_md)
            .unwrap()
            .contains("name: fuyao-skills-hub"),
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

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["init"]);
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
