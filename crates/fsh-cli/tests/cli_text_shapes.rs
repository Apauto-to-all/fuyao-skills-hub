//! 文本模式钉板：成功命令的 stdout 人读形态逐字钉住，输出层重构不得漂移。
//! --json 信封契约由其余集成测试覆盖；这里只断言不带 --json 的 stdout。

mod common;

use common::{env_with_source, fsh, hub_root};

/// 预演动作清单：逐行 `- ` 前缀，动作文本带 JSON 字符串引号
#[test]
fn text_dry_run_lists_actions_line_by_line() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);
    let src_s = src.display().to_string();

    // 重键预演：与实跑同样报错退出 1，不出动作清单
    let (out, code) = fsh(&["--hub", &hub, "add", &src_s, "--dry-run"]);
    assert_eq!(code, 1, "重键预演也应失败: {out}");

    // 不与库冲突的键才能拿到动作清单：前缀换新键位
    let (out, code) = fsh(&["--hub", &hub, "add", &src_s, "--prefix", "p", "--dry-run"]);
    assert_eq!(code, 0, "add --dry-run 文本: {out}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            format!(
                "- {}",
                serde_json::json!(format!("copy {src_s} -> store/p/demo"))
            ),
            format!("- {}", serde_json::json!("commit: add(store): p/demo")),
        ],
        "add 预演动作逐行钉住"
    );

    // archive 预演：三步动作逐行钉
    let (out, code) = fsh(&["--hub", &hub, "archive", "demo", "--dry-run"]);
    assert_eq!(code, 0, "archive --dry-run 文本: {out}");
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            format!("- {}", serde_json::json!("move store/demo -> archive/demo")),
            format!("- {}", serde_json::json!("commit: archive(store): demo")),
        ]
    );
}

/// 动作形态的判定只认命令预演，不认键名：实跑结果即使带数组字段也走 pretty
#[test]
fn text_mode_prints_pretty_json_for_plain_payloads() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]);

    // 已建 hub 的幂等预演：无动作清单，pretty JSON 直出
    let (out, code) = fsh(&["--hub", &hub, "init", "--dry-run"]);
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

    // list：文本模式逐行平铺（description 截断在显示层），无信封无 ok 键
    let (out, code) = fsh(&["--hub", &hub, "list"]);
    assert_eq!(code, 0, "list 文本: {out}");
    assert!(
        out.lines().any(|l| l.starts_with("demo  演示")),
        "list 文本应一行一个技能: {out}"
    );
    assert!(!out.contains("\"ok\""), "文本模式不得带信封键: {out}");
}
