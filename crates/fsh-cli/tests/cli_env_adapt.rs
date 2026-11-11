//! 环境变量适配的端到端验证：全局落点随专有变量 / XDG 根 / 存在性探测
//! 漂移，未设变量时回落家目录字面量；agents 回显保持配置原文不随环境漂移。
//! 环境变量一律经 fsh_env 注入给子进程，不改本进程环境；家目录与变量
//! 目录都用临时目录，守卫持有到测试结束。

mod common;

use common::{data_of, env_with_metadata_skill, fsh, fsh_env, home_env, initialized_hub};

/// 目标 JSON 的 destinations 里是否出现某个目录路径：逐条取 path 字段，
/// 分隔符统一成 `/` 再比，跨平台路径混写不影响
fn mentions(target: &serde_json::Value, dir: &std::path::Path) -> bool {
    let want = dir.display().to_string().replace('\\', "/");
    target["destinations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| d["path"].as_str())
        .any(|p| p.replace('\\', "/").contains(&want))
}

#[test]
fn claude_config_dir_moves_global_landing_in_both_dry_run_and_real_run() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = initialized_hub(&install, &src);
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let config = tempfile::tempdir().unwrap();

    // 预演：落点在变量目录下，且零落盘
    let config_s = config.path().display().to_string();
    let env: Vec<(&str, String)> = home_env(&home_s)
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .chain([("CLAUDE_CONFIG_DIR", config_s)])
        .collect();
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (out, code) = fsh_env(
        &env,
        &[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--global",
            "--agent",
            "claude-code",
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "use --dry-run: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    assert!(
        mentions(&d["targets"][0], &config.path().join("skills")),
        "预演落点应在 CLAUDE_CONFIG_DIR 下: {d}"
    );
    assert!(!config.path().join("skills").exists(), "预演零落盘");

    // 实铺：变量目录下技能入口存在，登记簿正常
    let (out, code) = fsh_env(
        &env,
        &[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--global",
            "--agent",
            "claude-code",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");
    let d = data_of(&out);
    assert!(
        mentions(&d["targets"][0], &config.path().join("skills")),
        "实铺落点应在 CLAUDE_CONFIG_DIR 下: {d}"
    );
    assert!(
        config.path().join("skills/demo/SKILL.md").is_file(),
        "变量目录下技能入口应存在"
    );
    assert!(
        !home.path().join(".claude").exists(),
        "设了变量就不得落在家目录默认位"
    );
    let registry = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert!(registry.contains("[[globals.skills]]"), "{registry}");
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list 登记正常: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let demo = rows
        .iter()
        .find(|r| r["key"] == "demo")
        .expect("demo 应在列表");
    assert_eq!(
        demo["usage_count"],
        serde_json::json!(1),
        "全局用法应计入引用计数: {out}"
    );
}

#[test]
fn without_var_global_landing_stays_on_home_literal() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = initialized_hub(&install, &src);
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();

    // 未设变量：预演落点在家目录字面量 ~/.claude/skills
    let (out, code) = fsh_env(
        &home_env(&home_s),
        &[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--global",
            "--agent",
            "claude-code",
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "use --dry-run: {out}");
    let d = data_of(&out);
    assert!(
        mentions(
            &d["targets"][0],
            &home.path().join(".claude").join("skills")
        ),
        "未设变量应落家目录默认位: {d}"
    );

    // 实铺确认真落在默认位
    let (out, code) = fsh_env(
        &home_env(&home_s),
        &[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo",
            "--global",
            "--agent",
            "claude-code",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");
    assert!(
        home.path().join(".claude/skills/demo/SKILL.md").is_file(),
        "未设变量时落点在家目录字面量"
    );
}

#[test]
fn xdg_config_home_moves_opencode_global_landing() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = initialized_hub(&install, &src);
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let xdg = tempfile::tempdir().unwrap();

    let xdg_s = xdg.path().display().to_string();
    let env: Vec<(&str, String)> = home_env(&home_s)
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .chain([("XDG_CONFIG_HOME", xdg_s)])
        .collect();
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (out, code) = fsh_env(
        &env,
        &[
            "--json", "--hub", &hub, "use", "demo", "--global", "--agent", "opencode",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");
    assert!(
        xdg.path().join("opencode/skills/demo/SKILL.md").is_file(),
        "XDG 根生效时落点在 $XDG_CONFIG_HOME/opencode/skills: {out}"
    );
    assert!(
        !home.path().join(".config").exists(),
        "设了 XDG 根就不得落家目录 .config"
    );
}

#[test]
fn openclaw_probe_picks_clawdbot_when_present() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = initialized_hub(&install, &src);
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    // 家目录只造 .clawdbot：存在性探测应选中它而非默认 .openclaw
    std::fs::create_dir_all(home.path().join(".clawdbot")).unwrap();

    let (out, code) = fsh_env(
        &home_env(&home_s),
        &[
            "--json", "--hub", &hub, "use", "demo", "--global", "--agent", "openclaw",
        ],
    );
    assert_eq!(code, 0, "use --global: {out}");
    assert!(
        home.path().join(".clawdbot/skills/demo/SKILL.md").is_file(),
        "存在 .clawdbot 时落点在 home/.clawdbot/skills: {out}"
    );
    assert!(
        !home.path().join(".openclaw").exists(),
        "探测命中 .clawdbot 就不得用默认 .openclaw"
    );
}

#[test]
fn agents_echo_stays_literal_with_env_vars_set() {
    let (install, src) = env_with_metadata_skill("1.0.0");
    let hub = initialized_hub(&install, &src);
    let home = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();

    let home_s = home.path().display().to_string();
    let config_s = config.path().display().to_string();
    let env: Vec<(&str, String)> = home_env(&home_s)
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .chain([("CLAUDE_CONFIG_DIR", config_s.clone())])
        .collect();
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (out, code) = fsh_env(&env, &["--json", "--hub", &hub, "agents"]);
    assert_eq!(code, 0, "agents: {out}");
    let rows = data_of(&out)["agents"].as_array().unwrap().clone();
    let claude = rows
        .iter()
        .find(|r| r["key"] == serde_json::json!("claude-code"))
        .expect("内置表含 claude-code");
    // 回显是配置原文（默认字面量），不随环境漂移
    assert_eq!(
        claude["global_skills_dir"],
        serde_json::json!("~/.claude/skills")
    );
    assert!(!out.contains(&config_s));
}
