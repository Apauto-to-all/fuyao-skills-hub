//! 被动版本检查与升级提示：提示行只出现在 stderr，stdout 与退出码一律不变。
//! 全程不联网——缓存由预写的状态文件给出，刷新一律指向不可达地址。

mod common;

use std::fs;
use std::path::Path;

use common::{
    Run, data_of, env_with_source, hub_root, init_hub_at, json, text, text_env, write_fresh_state,
    write_update_config,
};

/// 提示行的唯一形状：agent 能用一条正则抓住它
const NOTICE_PREFIX: &str = "提示: fuyao-skills-hub 有新版本 ";

/// 不可达发布源：连不上是瞬间的事，不必等三秒超时
const UNREACHABLE: &str = "http://127.0.0.1:1";

/// 起一个已 init 的 hub 根
fn hub() -> (tempfile::TempDir, String) {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    (install, hub)
}

/// stderr 里那行提示（没有则 None）
fn notice(run: &Run) -> Option<&str> {
    run.stderr
        .lines()
        .find(|line| line.starts_with(NOTICE_PREFIX))
}

#[test]
fn fresh_cache_with_a_newer_version_notifies_without_touching_stdout_or_code() {
    let (_install, hub) = hub();
    // 子进程的 stderr 是管道：要看到提示必须显式打开
    write_update_config(&hub, "notify_when_piped = true\n");
    write_fresh_state(&hub, Some("9.9.9"));

    let run = text(&hub, &["list"]);
    assert_eq!(run.code, 0, "提示不得改退出码: {}", run.stderr);
    assert_eq!(
        notice(&run).unwrap(),
        "提示: fuyao-skills-hub 有新版本 9.9.9（当前 0.1.2），升级: npm i -g fuyao-skills-hub@latest"
    );
    // stdout 是契约：提示绝不掺进去
    assert!(
        !run.stdout.contains("提示"),
        "stdout 不该有提示: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("builtin/fuyao-skills-hub"),
        "stdout 照常"
    );
}

#[test]
fn json_keeps_the_envelope_clean() {
    let (_install, hub) = hub();
    write_update_config(&hub, "notify_when_piped = true\n");
    write_fresh_state(&hub, Some("9.9.9"));

    let run = json(&hub, &["list"]);
    assert_eq!(run.code, 0);
    // 信封仍是一行可解析的 JSON，且没有提示行
    data_of(&run.stdout);
    assert_eq!(notice(&run), None, "stderr: {}", run.stderr);
}

#[test]
fn a_newer_version_does_not_disturb_the_json_envelope() {
    let (_install, hub) = hub();
    write_update_config(&hub, "notify_when_piped = true\n");
    write_fresh_state(&hub, Some("9.9.9"));
    let with_notice = json(&hub, &["list"]);

    write_update_config(&hub, "mode = \"off\"\n");
    let without = json(&hub, &["list"]);
    assert_eq!(
        with_notice.stdout, without.stdout,
        "提示开关不得影响 stdout 一个字节"
    );
}

#[test]
fn a_cached_version_that_is_not_newer_stays_silent() {
    let (_install, hub) = hub();
    write_update_config(&hub, "notify_when_piped = true\n");
    // 与当前同版、以及更旧的版本都不构成「有新版本」
    write_fresh_state(&hub, Some("0.1.2"));
    assert_eq!(notice(&text(&hub, &["list"])), None);

    write_fresh_state(&hub, Some("0.0.1"));
    assert_eq!(notice(&text(&hub, &["list"])), None);
}

#[test]
fn mode_off_neither_notifies_nor_queries() {
    let (_install, hub) = hub();
    write_update_config(
        &hub,
        &format!(
            "mode = \"off\"\nnotify_when_piped = true\nregistry = \"{UNREACHABLE}\"\ninterval_hours = 0\n"
        ),
    );
    // 状态过期且间隔为 0：只要查了就会改写 last_check
    write_fresh_state(&hub, Some("9.9.9"));
    let before = fs::read_to_string(Path::new(&hub).join("update.toml")).unwrap();

    let run = text(&hub, &["list"]);
    assert_eq!(run.code, 0);
    assert_eq!(notice(&run), None, "off 档不提示: {}", run.stderr);
    assert_eq!(
        fs::read_to_string(Path::new(&hub).join("update.toml")).unwrap(),
        before,
        "off 档连状态都不该动"
    );
}

#[test]
fn mode_manual_stays_silent_on_the_passive_path() {
    let (_install, hub) = hub();
    write_update_config(&hub, "mode = \"manual\"\nnotify_when_piped = true\n");
    write_fresh_state(&hub, Some("9.9.9"));
    assert_eq!(notice(&text(&hub, &["list"])), None);
}

#[test]
fn a_failing_command_gets_no_notice() {
    let (_install, hub) = hub();
    write_update_config(&hub, "notify_when_piped = true\n");
    write_fresh_state(&hub, Some("9.9.9"));

    // 点名的技能不存在：失败信封 + 退出码 1
    let run = text(&hub, &["archive", "--prefix", "k", "--skill", "nope"]);
    assert_eq!(run.code, 1);
    assert_eq!(notice(&run), None, "失败的命令不该带提示: {}", run.stderr);
}

#[test]
fn piped_stderr_is_silent_until_opted_in() {
    let (_install, hub) = hub();
    write_fresh_state(&hub, Some("9.9.9"));

    // 缺省 notify_when_piped = false：管道里不提示
    let run = text_env(&[("CI", "")], &hub, &["list"]);
    assert_eq!(run.code, 0);
    assert_eq!(notice(&run), None, "管道里默认闭嘴: {}", run.stderr);

    write_update_config(&hub, "notify_when_piped = true\n");
    write_fresh_state(&hub, Some("9.9.9"));
    let run = text_env(&[("CI", "")], &hub, &["list"]);
    assert!(notice(&run).is_some(), "显式打开后管道里也提示");
}

#[test]
fn ci_is_silent_until_opted_in() {
    let (_install, hub) = hub();
    write_update_config(&hub, "notify_when_piped = true\n");
    write_fresh_state(&hub, Some("9.9.9"));

    // CI 非空且 notify_in_ci = false（缺省）：不提示
    let run = text_env(&[("CI", "true")], &hub, &["list"]);
    assert_eq!(run.code, 0);
    assert_eq!(notice(&run), None, "CI 里默认不提示: {}", run.stderr);

    write_update_config(&hub, "notify_when_piped = true\nnotify_in_ci = true\n");
    write_fresh_state(&hub, Some("9.9.9"));
    let run = text_env(&[("CI", "true")], &hub, &["list"]);
    assert!(notice(&run).is_some(), "显式打开后 CI 里也提示");
}

#[test]
fn an_expired_cache_queries_once_and_fails_silently() {
    let (_install, hub) = hub();
    write_update_config(
        &hub,
        &format!("notify_when_piped = true\nregistry = \"{UNREACHABLE}\"\ninterval_hours = 24\n"),
    );
    // 从未查过：节流到期，这次会真的去查
    let run = text_env(&[("CI", "")], &hub, &["list"]);
    assert_eq!(run.code, 0, "连不上也要照常成功: {}", run.stderr);
    assert_eq!(notice(&run), None, "查不到就静默放弃");

    // 失败的检查也记时间：下一条命令不再等一次超时
    let state = fs::read_to_string(Path::new(&hub).join("update.toml")).unwrap();
    assert!(
        state.contains("last_check"),
        "失败的检查也要写检查时间: {state}"
    );
    assert!(!state.contains("latest"), "查不到不该编出一个版本: {state}");
}

#[test]
fn the_state_file_lives_in_the_hub_root_and_stays_out_of_git() {
    let (_install, hub) = hub();
    // 直接落一份状态文件（不改 hub.toml，工作区本该干净），再跑一条命令
    write_fresh_state(&hub, Some("9.9.9"));
    let run = text_env(&[("CI", "")], &hub, &["list"]);
    assert_eq!(run.code, 0);

    assert!(
        Path::new(&hub).join("update.toml").is_file(),
        "状态文件落在 hub 根"
    );
    let repo = fsh_core::git::open_hub(Path::new(&hub)).unwrap();
    assert!(
        !fsh_core::git::is_dirty(&repo).unwrap(),
        "机器写的时间戳不该进仓库、不该触发自动提交"
    );
}

#[test]
fn a_broken_hub_toml_falls_back_to_defaults_and_commands_still_work() {
    let (_install, hub) = hub();
    // 写坏配置：只有这一项功能回退缺省值，命令照常
    fs::write(Path::new(&hub).join("hub.toml"), "schema_version = \n").unwrap();
    write_fresh_state(&hub, Some("9.9.9"));

    let run = text(&hub, &["list"]);
    assert_eq!(run.code, 0, "坏配置不该让命令失败: {}", run.stderr);
    assert!(
        run.stdout.contains("builtin/fuyao-skills-hub"),
        "命令结果照常: {}",
        run.stdout
    );
    // 缺省档位是 auto、缺省 notify_when_piped = false：管道里安静
    assert_eq!(notice(&run), None);
    // 只记一条 WARN，不重复记：grep 结果不能失真（WARN 进 hub 根的日志文件）
    let log = fs::read_dir(Path::new(&hub).join("logs"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "log"))
        .map(|entry| fs::read_to_string(entry.path()).unwrap_or_default())
        .collect::<String>();
    assert_eq!(
        log.matches("版本检查配置降级").count(),
        1,
        "坏配置只该有一条 WARN: {log}"
    );
}
