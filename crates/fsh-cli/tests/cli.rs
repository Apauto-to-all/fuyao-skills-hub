//! Seam 2 集成测试：起真实 fsh 子进程，断言 --json 信封、退出码 0/1、
//! --dry-run 零落盘。外部行为契约在此固化，不测任何内部实现。

use std::path::{Path, PathBuf};

use serde_json::Value;

mod common;

use common::{data_of, fsh};

/// 解析失败信封，断言 ok:false 并返回 error 节点
fn error_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(false), "信封: {v}");
    v["error"].clone()
}

/// 建测试环境：临时安装目录（hub 根 = <install>/data）+ 一个待入库 skill 源目录
/// （目录名必须等于 frontmatter name，故用 demo）
fn env_with_source() -> (tempfile::TempDir, PathBuf) {
    let install = tempfile::tempdir().unwrap();
    let src = install.path().join("demo");
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
    // 三个空目录一次到位：库、导出、agent 覆盖都留着待用
    for dir in ["store", "export", "agents"] {
        assert!(
            install.path().join("data").join(dir).is_dir(),
            "{dir}/ 应由 init 建出"
        );
    }

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
fn add_list_show_release_diff_full_flow() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    let (_, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0);

    // add：一步到位打出首版 tag
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    assert_eq!(code, 0, "add: {out}");
    assert_eq!(data_of(&out)["key"], serde_json::json!("demo"));
    assert_eq!(data_of(&out)["version"], serde_json::json!("0.1.0"));

    // list：薄行含归档态
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0);
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["key"], serde_json::json!("demo"));
    assert_eq!(rows[0]["archived"], serde_json::json!(false));

    // show：全量详情含版本时间线
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 0);
    let d = data_of(&out);
    assert_eq!(d["versions"], serde_json::json!(["0.1.0"]));

    // 编辑工作副本（号写在文件里）后发版
    let work = install.path().join("data/store/demo/SKILL.md");
    std::fs::write(
        &work,
        "---\nname: demo\ndescription: 演示\nversion: \"0.2.0\"\n---\n\n新正文\n",
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "demo"]);
    assert_eq!(code, 0, "release: {out}");
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

    // 文本模式：直接输出 unified patch 原文，不打包 JSON 信封
    let (out, _) = fsh(&["--hub", &hub, "diff", "demo", "0.1.0", "0.2.0"]);
    assert!(out.contains("+新正文"), "文本 diff 应含 +新正文: {out}");
    assert!(out.contains("diff SKILL.md"));
    assert!(!out.starts_with('{'), "文本模式不得输出 JSON 信封");
}

#[test]
fn archive_restore_remove_lifecycle() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);

    // 归档：list 标 archived，tag 全留（show 仍可看）
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "demo"]);
    assert_eq!(code, 0, "archive: {out}");
    let (out, _) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(
        data_of(&out)["skills"][0]["archived"],
        serde_json::json!(true)
    );

    // restore：编辑面复活
    let (out, code) = fsh(&["--json", "--hub", &hub, "restore", "demo"]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(install.path().join("data/store/demo/SKILL.md").is_file());

    // remove：无人引用时连 tag 一起删，之后 show 报 skill_not_found
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo"]);
    assert_eq!(code, 0, "remove: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("skill_not_found"));
}

#[test]
fn remove_referenced_by_project_is_rejected() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    let project = tempfile::tempdir().unwrap();
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    // 登记簿记一条项目引用（use 之后的状态）；路径用 TOML 字面量串，
    // Windows 反斜杠无需转义
    let registry = install.path().join("data/projects.toml");
    std::fs::write(
        &registry,
        format!(
            "[[projects]]\npath = '{}'\nlast_used = 1\n\n[[projects.skills]]\nkey = \"demo\"\nversion = \"0.1.0\"\nagents = [\"agents\"]\nlink = \"junction\"\n",
            project.path().display()
        ),
    )
    .unwrap();

    // 被引用的 skill 不许删：失败信封 + 退出码 1
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo"]);
    assert_eq!(code, 1, "remove 被引用: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("referenced"));

    // 归档同样拒绝，原因同一份登记簿
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "demo"]);
    assert_eq!(code, 1, "archive 被引用: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("referenced"));
}

#[test]
fn dry_runs_plan_without_touching_disk() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
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

    // release / archive / remove --dry-run：动作清单各就各位，盘上一字未动
    let (_, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
    assert_eq!(code, 0);
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "demo", "--dry-run"]);
    assert_eq!(code, 0, "archive dry: {out}");
    assert!(
        data_of(&out)["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "remove store/demo")
    );
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo", "--dry-run"]);
    assert_eq!(code, 0, "remove dry: {out}");
    assert!(
        data_of(&out)["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a == "delete tag demo/v0.1.0")
    );
    assert!(install.path().join("data/store/demo/SKILL.md").is_file());

    // gc --dry-run / init --dry-run / backup --dry-run 语义存在且退出码 0
    let (_, code) = fsh(&["--json", "--hub", &hub, "gc", "--dry-run"]);
    assert_eq!(code, 0);
    let (_, code) = fsh(&["--json", "--hub", &hub, "init", "--dry-run"]);
    assert_eq!(code, 0);
    let dest = tempfile::tempdir().unwrap();
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "backup",
        &dest.path().display().to_string(),
        "--dry-run",
    ]);
    assert_eq!(code, 0, "backup dry: {out}");
    // 动作清单反映真实将执行的动作：真实 zip 名与登记簿回写
    let actions = data_of(&out)["actions"].as_array().unwrap().clone();
    assert!(actions[0].as_str().unwrap().contains("fuyao-backup-"));
    assert!(actions[0].as_str().unwrap().contains(".zip"));
    assert!(
        actions
            .iter()
            .any(|a| a.as_str().unwrap().contains("projects.toml"))
    );
    assert!(dest.path().read_dir().unwrap().next().is_none());
}

#[test]
fn doctor_reports_checks_and_envelope() {
    let (install, _) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    let (out, code) = fsh(&["--json", "--hub", &hub, "doctor"]);
    // 临时目录环境：八项检查、无 FAIL → 退出码 0
    assert_eq!(code, 0, "doctor: {out}");
    let d = data_of(&out);
    assert_eq!(d["checks"].as_array().unwrap().len(), 8);
    assert_eq!(d["fail"], serde_json::json!(false));
}

#[test]
fn add_with_prefix_uses_nested_key() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);

    // --prefix 提供组织前缀：键 = 前缀/frontmatter name
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "add",
        &src.display().to_string(),
        "--prefix",
        "anthropics/skills",
    ]);
    assert_eq!(code, 0, "add --prefix: {out}");
    assert_eq!(
        data_of(&out)["key"],
        serde_json::json!("anthropics/skills/demo")
    );

    // 嵌套键在 list 里出现，后续命令按键寻址
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0);
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    assert!(
        rows.iter()
            .any(|r| r["key"] == serde_json::json!("anthropics/skills/demo")),
        "list 应含嵌套键: {rows:?}"
    );
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "anthropics/skills/demo"]);
    assert_eq!(code, 0, "show 嵌套键: {out}");
    assert_eq!(data_of(&out)["versions"], serde_json::json!(["0.1.0"]));
}

#[test]
fn agents_lists_builtin_and_user_configs() {
    let (install, _) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    std::fs::write(
        install.path().join("data/agents/codex.toml"),
        "display_name = \"My Codex\"\nskills_dir = \"custom/codex\"\n",
    )
    .unwrap();
    std::fs::write(
        install.path().join("data/agents/my-agent.toml"),
        "skills_dir = \".my-agent/skills\"\n",
    )
    .unwrap();

    let (out, code) = fsh(&["--json", "--hub", &hub, "agents"]);
    assert_eq!(code, 0, "agents: {out}");
    let rows = data_of(&out)["agents"].as_array().unwrap().clone();
    let find = |key: &str| {
        rows.iter()
            .find(|r| r["key"] == serde_json::json!(key))
            .unwrap_or_else(|| panic!("缺 agent {key}: {rows:?}"))
    };
    assert_eq!(
        find("dsh")["display_name"],
        serde_json::json!("DeepSeek Harness")
    );
    assert_eq!(find("dsh")["skills_dir"], serde_json::json!(".dsh/skills"));
    assert_eq!(find("dsh")["source"], serde_json::json!("built_in"));
    assert_eq!(find("codex")["display_name"], serde_json::json!("My Codex"));
    assert_eq!(
        find("codex")["skills_dir"],
        serde_json::json!("custom/codex")
    );
    assert_eq!(find("codex")["source"], serde_json::json!("user"));
    assert_eq!(
        find("my-agent")["display_name"],
        serde_json::json!("my-agent")
    );
    assert_eq!(rows.len(), 82, "81 项内置加 1 项新增");
}

#[test]
fn invalid_agent_config_fails_with_stable_code() {
    let (install, _) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    std::fs::write(
        install.path().join("data/agents/bad.toml"),
        "skills_dir = \"../escape\"\n",
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "agents"]);
    assert_eq!(code, 1, "非法配置应失败: {out}");
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("invalid_agent_config")
    );
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

#[test]
fn help_lists_the_thirteen_commands() {
    let (out, code) = fsh(&["--help"]);
    assert_eq!(code, 0);
    // 帮助的 Commands 区块逐行一条；末行 help 是 clap 自带的子命令
    let listed: Vec<&str> = out
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(listed.len(), 14, "命令面应恰好 13 条加 help: {listed:?}");
    for kept in [
        "init", "add", "list", "show", "release", "archive", "remove", "restore", "agents", "diff",
        "gc", "doctor", "backup",
    ] {
        assert!(listed.contains(&kept), "帮助里应有 {kept}: {out}");
    }
}
