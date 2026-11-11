//! 文本模式钉板：成功命令的 stdout 人读形态逐字钉住，输出层重构不得漂移。
//! --json 信封契约由其余集成测试覆盖；这里只断言不带 --json 的 stdout。

mod common;

use common::{
    Run, data_of, env_with_source, error_of, home_env, hub_root, init_hub, init_hub_at, json,
    json_env, text,
};

/// 预演动作清单：逐行 `- ` 前缀，动作文本按「动作词 + 路径」，不带引号
#[test]
fn text_dry_run_lists_actions_line_by_line() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    json(&hub, &["add", &src.display().to_string()]);
    let src_s = src.display().to_string();

    // 重键预演：与实跑同样报错退出 1，不出动作清单
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["add", &src_s, "--dry-run"]);
    assert_eq!(code, 1, "重键预演也应失败: {out}");

    // 不与库冲突的键才能拿到动作清单：前缀换新键位
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["add", &src_s, "--prefix", "p", "--dry-run"]);
    assert_eq!(code, 0, "add --dry-run 文本: {out}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            format!("- copy {src_s} -> store/p/demo"),
            "- commit: add(store): p/demo".to_string(),
        ],
        "add 预演动作逐行钉住"
    );

    // archive 预演：动作逐行钉
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["archive", "demo", "--dry-run"]);
    assert_eq!(code, 0, "archive --dry-run 文本: {out}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            "- move store/demo -> archive/demo".to_string(),
            "- commit: archive(store): demo".to_string(),
        ]
    );
}

/// 动作形态的判定只认命令预演，不认键名：实跑结果即使带数组字段也走 pretty
#[test]
fn text_mode_prints_pretty_json_for_plain_payloads() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    json(&hub, &["add", &src.display().to_string()]);

    // 已建 hub 的幂等预演：无动作清单，pretty JSON 直出
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["init", "--dry-run"]);
    assert_eq!(code, 0, "init --dry-run 文本: {out}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            "{",
            "  \"already_initialized\": true,",
            "  \"dry_run\": true",
            "}"
        ],
        "无动作清单的结果按 pretty JSON 出"
    );

    // list：文本模式按面分块（面标题 + 空行 + 一块一个技能），无信封无 ok 键
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["list", "demo"]);
    assert_eq!(code, 0, "list 文本: {out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Store skills", "面标题: {out}");
    assert_eq!(lines[1], "", "标题后空一行: {out}");
    let at = lines.iter().position(|l| *l == "demo").expect("demo 块");
    assert_eq!(lines[at + 1], "  Description: 演示", "字段行: {out}");
    assert!(!out.contains("\"ok\""), "文本模式不得带信封键: {out}");
}

/// list 文本形态钉住：技能块之间空行、无 source 与无 uses 时整行不出、
/// 使用清单逐记录一行且列在块内对齐
#[test]
fn text_list_renders_blocks_with_aligned_uses() {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    // 带 source 的技能：一条项目记录 + 一条全局记录，记录内两个 agent
    let src = install.path().join("demo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: demo description\nmetadata:\n  source: demo-src\n---\n\n正文\n",
    )
    .unwrap();
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "demo",
                "--agent",
                "universal,dsh",
                "--project",
                &proj
            ]
        )
        .code,
        0
    );
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    assert_eq!(
        json_env(
            &home_env(&home_s),
            &hub,
            &["use", "demo", "--agent", "universal,dsh", "--global"]
        )
        .code,
        0
    );

    // JSON：一条登记记录一格，字段 scope / project / link / agents
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list", "demo"]);
    assert_eq!(code, 0, "{out}");
    let uses = data_of(&out)["skills"][0]["uses"].clone();
    let uses = uses.as_array().unwrap();
    assert_eq!(uses.len(), 2, "全局一条、项目一条: {uses:?}");
    assert_eq!(uses[0]["scope"], serde_json::json!("global"));
    assert!(uses[0].get("project").is_none(), "全局记录不写项目");
    assert_eq!(uses[1]["scope"], serde_json::json!("project"));
    assert_eq!(uses[1]["agents"], serde_json::json!(["universal", "dsh"]));
    assert!(uses[1]["project"].is_string(), "{uses:?}");
    let link = uses[1]["link"].as_str().unwrap().to_string();

    // 文本：块首键、字段行、使用清单逐记录一行
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["list", "demo"]);
    assert_eq!(code, 0, "list 文本: {out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Store skills", "面标题: {out}");
    assert_eq!(lines[1], "", "标题后空一行: {out}");
    assert_eq!(lines[2], "demo", "键原样一行: {out}");
    assert_eq!(lines[3], "  Description: demo description", "{out}");
    assert_eq!(lines[4], "  Source:      demo-src", "{out}");
    assert!(
        lines[5].starts_with("  Uses:        "),
        "使用清单首行带 Uses 标签: {out}"
    );
    assert!(
        lines[6].starts_with("               ") && !lines[6].contains("Uses:"),
        "续行用等宽空白接着列: {out}"
    );
    for line in [lines[5], lines[6]] {
        assert!(line.contains(&link), "链接方式逐行带出: {out}");
        assert!(
            line.contains("universal, dsh"),
            "同记录多 agent 逗号并列: {out}"
        );
    }
    // 对齐：两行的链接列与 agent 列起点一致（目标长度不同，只补空格）
    let column = |line: &str, needle: &str| line.find(needle).unwrap();
    assert_eq!(column(lines[5], &link), column(lines[6], &link), "{out}");
    assert_eq!(
        column(lines[5], "universal, dsh"),
        column(lines[6], "universal, dsh"),
        "{out}"
    );
    // 全局目标写 global，项目目标写登记的项目路径
    assert!(lines[5].contains("global"), "{out}");
    let target_name = project
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        lines[6].contains(&target_name),
        "项目目标写登记的项目路径: {out}"
    );

    // 无 source、无使用的技能：两条行都不出，块间不留尾随空行
    let plain = install.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::write(
        plain.join("SKILL.md"),
        "---\nname: plain\ndescription: 无来源\n---\n\n正文\n",
    )
    .unwrap();
    assert_eq!(json(&hub, &["add", &plain.display().to_string()]).code, 0);
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["list", "plain"]);
    assert_eq!(code, 0, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4, "标题 + 空行 + 键 + 字段行: {out}");
    assert_eq!(lines[0], "Store skills");
    assert_eq!(lines[2], "plain");
    assert_eq!(lines[3], "  Description: 无来源");
    assert!(
        !out.contains("Source:") && !out.contains("Uses:"),
        "无来源与无使用时整行不出: {out}"
    );

    // 归档面换标题
    assert_eq!(json(&hub, &["archive", "plain"]).code, 0);
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["list", "plain", "--archived"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.lines().next(), Some("Archived skills"), "{out}");
}

/// 失败输出分两路：文本模式只进 stderr 且 stdout 不写字，`--json` 模式只进 stdout 信封
#[test]
fn failure_output_goes_to_stderr_only_in_text_mode() {
    let install = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);

    // 文本模式：人类可读的错误行只在 stderr，stdout 保持空
    let text_run = text(&hub, &["use", "ghost", "--agent", "universal", "--global"]);
    assert_eq!(text_run.code, 1);
    assert_eq!(
        text_run.stdout, "",
        "文本模式失败不得污染 stdout: {}",
        text_run.stdout
    );
    assert!(
        text_run.stderr.contains("错误 [skill_not_found]"),
        "文本模式失败必须在 stderr 点名稳定码: {}",
        text_run.stderr
    );

    // --json 模式：错误只进 stdout 信封，stderr 不再重复一行错误
    let json_run = json(&hub, &["use", "ghost", "--agent", "universal", "--global"]);
    assert_eq!(json_run.code, 1);
    assert_eq!(error_of(&json_run.stdout)["code"], "skill_not_found");
    assert!(
        !json_run.stderr.contains("错误 ["),
        "--json 模式不得在 stderr 重复错误行: {}",
        json_run.stderr
    );
}
