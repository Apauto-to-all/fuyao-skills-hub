//! Seam 2 集成测试：起真实 fsh 子进程，断言 --json 信封、退出码 0/1 与磁盘真实效果。
//! 外部行为契约在此固化，不测任何内部实现。

mod common;

use std::path::Path;

use common::{data_of, env_with_metadata_skill, env_with_source, error_of, fsh, hub_root};
#[test]
fn init_is_enveloped_and_idempotent() {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0);
    assert_eq!(data_of(&out)["initialized"], serde_json::json!(true));
    // 三个空目录一次到位：库、导出、agent 覆盖都留着待用
    for dir in ["store", "archive", "agents"] {
        assert!(install.path().join(dir).is_dir(), "{dir}/ 应由 init 建出");
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
    let hub_toml = install.path().join("hub.toml");
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
    let (out, code) = fsh(&["--json", "--hub", &hub_root(empty.path()), "list"]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("hub_not_initialized")
    );
}

#[test]
fn add_list_full_flow() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let (_, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0);

    // add：一步到位入库
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    assert_eq!(code, 0, "add: {out}");
    assert_eq!(data_of(&out)["key"], serde_json::json!("demo"));

    // list：一行一个技能，JSON 给 description 全文
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let demo = rows.iter().find(|r| r["key"] == "demo").unwrap();
    assert_eq!(demo["description"], serde_json::json!("演示"));
    assert_eq!(demo["usage_count"], serde_json::json!(0));
    // 位置过滤词收窄
    let (out, code) = fsh(&["--json", "--hub", &hub, "list", "demo"]);
    assert_eq!(code, 0);
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    assert!(rows.iter().all(|r| r["key"] == "demo"), "过滤后只剩 demo");
}

#[test]
fn archive_remove_lifecycle() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);

    // 归档：工作副本整目录移入归档面
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "demo"]);
    assert_eq!(code, 0, "archive: {out}");
    assert!(!install.path().join("store/demo/SKILL.md").exists());
    assert!(
        install.path().join("archive/demo/SKILL.md").is_file(),
        "归档副本应落在 archive/demo"
    );
    // 归档技能只在归档旗标下列出
    let (out, _) = fsh(&["--json", "--hub", &hub, "list"]);
    let keys = |out: &str| {
        data_of(out)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["key"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert!(!keys(&out).contains(&"demo".to_string()), "缺省不列归档");
    let (archived_out, _) = fsh(&["--json", "--hub", &hub, "list", "--archived"]);
    assert!(keys(&archived_out).contains(&"demo".to_string()));

    // 恢复：移回 store
    let (out, code) = fsh(&["--json", "--hub", &hub, "restore", "demo"]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(install.path().join("store/demo/SKILL.md").is_file());
    assert!(!install.path().join("archive/demo").exists());

    // remove 成功后列表与归档面都没有它
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo"]);
    assert_eq!(code, 0, "remove: {out}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list", "--archived"]);
    assert_eq!(code, 0);
    assert!(!keys(&out).contains(&"demo".to_string()));
}

#[test]
fn remove_referenced_by_project_is_rejected() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    // 登记簿记一条项目引用（use 之后的状态）；路径用 TOML 字面量串，
    // Windows 反斜杠无需转义
    let registry = install.path().join("projects.toml");
    let project_path = project.path().display().to_string();
    std::fs::write(
        &registry,
        format!(
            "[projects.'{project_path}']\nlast_used = 1\n\n[[projects.'{project_path}'.skills]]\nkey = \"demo\"\nagents = [\"agents\"]\nlink = \"junction\"\n"
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
fn add_with_prefix_uses_nested_key() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);

    // --prefix 提供组织前缀：键 = 前缀/frontmatter name
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "add",
        &src.display().to_string(),
        "--prefix",
        "mattpocock/skills",
    ]);
    assert_eq!(code, 0, "add --prefix: {out}");
    assert_eq!(
        data_of(&out)["key"],
        serde_json::json!("mattpocock/skills/demo")
    );

    // 嵌套键出现在列表里
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let nested = rows
        .iter()
        .find(|r| r["key"] == "mattpocock/skills/demo")
        .expect("嵌套键应出现在列表");
    assert_eq!(nested["description"], serde_json::json!("演示"));
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

/// init 的帮助要说清幂等语义：已初始化只报告，内容一致则不动
#[test]
fn init_help_says_content_matches_stays_untouched() {
    let (out, code) = fsh(&["init", "--help"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("内容一致则不动"),
        "init 帮助应写明内容一致则不动: {out}"
    );
}

/// unuse 的帮助要说清两种撤回的分别，`--agent` 出现且带整体撤回的说明
#[test]
fn unuse_help_explains_agent_scoped_and_whole_withdrawal() {
    let (out, code) = fsh(&["unuse", "--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("--agent"), "帮助里应有 --agent: {out}");
    assert!(
        out.contains("按 agent 撤") && out.contains("不给则整体撤回"),
        "帮助要说清给与不给 --agent 的区别: {out}"
    );
}

#[test]
fn help_lists_the_twelve_commands() {
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
    assert_eq!(listed.len(), 13, "命令面应恰好 12 条加 help: {listed:?}");
    for kept in [
        "init", "add", "list", "archive", "remove", "restore", "agents", "use", "unuse", "search",
        "doctor", "projects",
    ] {
        assert!(listed.contains(&kept), "帮助里应有 {kept}: {out}");
    }
}

#[test]
fn projects_lists_registered_skill_and_missing_destination() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let src_s = src.display().to_string();

    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(fsh(&["--json", "--hub", &hub, "add", &src_s]).1, 0);
    assert_eq!(
        fsh(&[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--agent",
            "universal",
            "--project",
            &proj
        ])
        .1,
        0
    );

    let (out, code) = fsh(&["--json", "--hub", &hub, "projects"]);
    assert_eq!(code, 0, "projects: {out}");
    let rows = data_of(&out)["projects"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    let skill = &rows[0]["skills"][0];
    assert_eq!(skill["key"], serde_json::json!("demo"));
    let dests = skill["destinations"].as_array().unwrap().clone();
    assert_eq!(dests.len(), 1);
    assert_eq!(dests[0]["exists"], serde_json::json!(true));

    // 手删落点：项目仍在册，落点标记缺失
    std::fs::remove_dir(dests[0]["path"].as_str().unwrap()).unwrap();
    let (out, _) = fsh(&["--json", "--hub", &hub, "projects"]);
    let dests = data_of(&out)["projects"][0]["skills"][0]["destinations"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(dests[0]["exists"], serde_json::json!(false));
}
