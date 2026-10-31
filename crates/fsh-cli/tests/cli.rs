//! Seam 2 集成测试：起真实 fsh 子进程，断言 --json 信封、退出码 0/1/2、
//! --dry-run 零落盘。外部行为契约在此固化，不测任何内部实现。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// 起一次 fsh，返回 (stdout, 退出码)
fn fsh(args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_fsh"))
        .args(args)
        .output()
        .expect("起 fsh 子进程失败");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// 解析成功信封，断言 ok:true
fn data_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(true), "信封: {v}");
    v["data"].clone()
}

/// 解析失败信封，断言 ok:false 并返回 error 节点
fn error_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(false), "信封: {v}");
    v["error"].clone()
}

/// 建测试环境：临时安装目录（hub 根 = <install>/data）+ 一个待入库 skill 源目录
fn env_with_source() -> (tempfile::TempDir, PathBuf) {
    let install = tempfile::tempdir().unwrap();
    let src = install.path().join("src-skill");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    (install, src)
}

/// hub 根路径（安装目录下 data\）
fn data_dir(install: &Path) -> String {
    install.join("data").display().to_string()
}

#[test]
fn init_is_enveloped_and_idempotent() {
    let (install, _) = env_with_source();
    let hub = data_dir(install.path());
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0);
    assert_eq!(data_of(&out)["initialized"], serde_json::json!(true));

    // 重跑幂等：ok:true + already_initialized，一字节不动
    let mtime = |p: &Path| {
        std::fs::metadata(p)
            .unwrap()
            .modified()
            .unwrap()
            .elapsed()
            .unwrap()
            .as_secs()
    };
    let hub_toml = install.path().join("data/hub.toml");
    let before = mtime(&hub_toml);
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0);
    assert_eq!(
        data_of(&out)["already_initialized"],
        serde_json::json!(true)
    );
    assert_eq!(mtime(&hub_toml), before, "幂等重跑不能改文件");

    // 未初始化的目录上直接跑业务命令 → 失败信封 + 退出码 1
    let empty = tempfile::tempdir().unwrap();
    let (out, code) = fsh(&["--json", "--hub", &data_dir(empty.path()), "list"]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("hub_not_initialized")
    );
}

#[test]
fn add_list_show_version_diff_full_flow() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    let (_, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0);

    // add：一步到位打出首版 tag
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    assert_eq!(code, 0, "add: {out}");
    assert_eq!(data_of(&out)["skill"], serde_json::json!("demo"));
    assert_eq!(data_of(&out)["version"], serde_json::json!("0.1.0"));

    // list：薄行含归档态
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0);
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], serde_json::json!("demo"));
    assert_eq!(rows[0]["archived"], serde_json::json!(false));

    // show：全量详情含版本时间线
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 0);
    let d = data_of(&out);
    assert_eq!(d["versions"], serde_json::json!(["0.1.0"]));

    // 编辑工作副本（模拟迭代）后发 minor 版
    let work = install.path().join("data/store/demo/SKILL.md");
    std::fs::write(&work, "---\nname: demo\ndescription: 演示\n---\n\n新正文\n").unwrap();
    let (out, code) = fsh(&[
        "--json", "--hub", &hub, "version", "demo", "--bump", "minor",
    ]);
    assert_eq!(code, 0, "bump: {out}");
    assert_eq!(data_of(&out)["version"], serde_json::json!("0.2.0"));

    // diff 两版：含 unified patch
    let (out, code) = fsh(&["--json", "--hub", &hub, "diff", "demo", "0.1.0", "0.2.0"]);
    assert_eq!(code, 0);
    let files = data_of(&out)["files"].as_array().unwrap().clone();
    let patch = files
        .iter()
        .find(|f| f["path"] == "SKILL.md")
        .expect("SKILL.md 应在 diff 里");
    assert!(patch["patch"].as_str().unwrap().contains("+新正文"));
}

#[test]
fn remove_restore_purge_lifecycle() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);

    // 默认归档：list 标 archived，tag 全留（show 仍可看）
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo"]);
    assert_eq!(code, 0, "remove: {out}");
    let (out, _) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(
        data_of(&out)["skills"][0]["archived"],
        serde_json::json!(true)
    );

    // restore：编辑面复活
    let (out, code) = fsh(&["--json", "--hub", &hub, "restore", "demo"]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(install.path().join("data/store/demo/SKILL.md").is_file());

    // purge：无人引用时连 tag 一起删，之后 show 报 skill_not_found
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo", "--purge"]);
    assert_eq!(code, 0, "purge: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("skill_not_found"));
}

#[test]
fn purge_referenced_by_project_is_rejected() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    let project = tempfile::tempdir().unwrap();
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    // 项目声明 + apply 铺链 → lock 引用 demo
    std::fs::write(
        project.path().join("skills.toml"),
        "[[skills]]\nname = \"demo\"\nversion = \"0.1.0\"\ntarget = \"claude-code\"\n",
    )
    .unwrap();
    let (_, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "apply",
        &project.path().display().to_string(),
    ]);
    assert_eq!(code, 0);

    // 被引用的 skill 不许 purge：失败信封 + 退出码 1
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo", "--purge"]);
    assert_eq!(code, 1, "purge 被引用: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("referenced"));
}

#[test]
fn status_reports_drift_with_exit_2() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    let project = tempfile::tempdir().unwrap();
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    std::fs::write(
        project.path().join("skills.toml"),
        "[[skills]]\nname = \"demo\"\nversion = \"0.1.0\"\ntarget = \"claude-code\"\n",
    )
    .unwrap();
    let proj = project.path().display().to_string();

    // 未 apply 前无账本基线，status 视为干净（漂移只对已铺链的 lock 负责）
    let (out, code) = fsh(&["--json", "--hub", &hub, "status", &proj]);
    assert_eq!(code, 0, "无 lock 基线应干净: {out}");
    assert_eq!(data_of(&out)["clean"], serde_json::json!(true));

    // apply 后干净 → 0
    let (_, code) = fsh(&["--json", "--hub", &hub, "apply", &proj]);
    assert_eq!(code, 0);
    let (out, code) = fsh(&["--json", "--hub", &hub, "status", &proj]);
    assert_eq!(code, 0, "apply 后应干净: {out}");
    assert_eq!(data_of(&out)["clean"], serde_json::json!(true));

    // 删链接再查 → 又漂移
    std::fs::remove_dir_all(project.path().join(".fsh/skills/demo")).unwrap();
    let (_, code) = fsh(&["--json", "--hub", &hub, "status", &proj]);
    assert_eq!(code, 2);
}

#[test]
fn dry_runs_plan_without_touching_disk() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    let project = tempfile::tempdir().unwrap();
    fsh(&["--json", "--hub", &hub, "init"]);
    std::fs::write(
        project.path().join("skills.toml"),
        "[[skills]]\nname = \"demo\"\nversion = \"0.1.0\"\ntarget = \"claude-code\"\n",
    )
    .unwrap();
    let proj = project.path().display().to_string();
    let src_s = src.display().to_string();

    // add --dry-run：动作清单齐全，store 一字未动
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s, "--dry-run"]);
    assert_eq!(code, 0, "add dry: {out}");
    let d = data_of(&out);
    let actions = d["actions"].as_array().unwrap().clone();
    assert!(
        actions
            .iter()
            .any(|a| a.as_str().unwrap().contains("tag demo/v0.1.0"))
    );
    assert!(
        install
            .path()
            .join("data/store")
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );

    // apply --dry-run（= plan）：报 Install 变更，不铺链不写 lock（先真入库 demo）
    let (_, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
    assert_eq!(code, 0);
    let (out, code) = fsh(&["--json", "--hub", &hub, "apply", &proj, "--dry-run"]);
    assert_eq!(code, 0, "apply dry: {out}");
    assert!(data_of(&out)["changes"].as_array().unwrap().len() == 1);
    assert!(!project.path().join("skills.lock.toml").exists());
    assert!(!project.path().join(".fsh").exists());

    // gc --dry-run / init --dry-run / backup --dry-run 语义存在且退出码 0
    let (_, code) = fsh(&["--json", "--hub", &hub, "gc", "--dry-run"]);
    assert_eq!(code, 0);
    let (_, code) = fsh(&["--json", "--hub", &hub, "init", "--dry-run"]);
    assert_eq!(code, 0);
    let dest = tempfile::tempdir().unwrap();
    let (_, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "backup",
        &dest.path().display().to_string(),
        "--dry-run",
    ]);
    assert_eq!(code, 0);
    assert!(dest.path().read_dir().unwrap().next().is_none());
}

#[test]
fn doctor_reports_checks_and_envelope() {
    let (install, _) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    let (out, code) = fsh(&["--json", "--hub", &hub, "doctor"]);
    // 临时目录环境：七项检查、无 FAIL → 退出码 0
    assert_eq!(code, 0, "doctor: {out}");
    let d = data_of(&out);
    assert_eq!(d["checks"].as_array().unwrap().len(), 7);
    assert_eq!(d["fail"], serde_json::json!(false));
}

#[test]
fn unknown_command_gets_error_envelope_with_exit_1() {
    let (out, code) = fsh(&["--json", "nope"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("usage"));

    // 文本模式下 --help 属正常输出，退出码 0
    let (_, code) = fsh(&["--help"]);
    assert_eq!(code, 0);
}
