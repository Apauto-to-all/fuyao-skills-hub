//! 被动检查的判定：抑制条件逐条、节流、失败也记时间、成功的检查无条件覆盖缓存。
//! 全部不联网——要么用预写的状态文件，要么把发布源指向不可达地址。

use std::fs;
use std::path::Path;

use super::*;
use crate::hub;

/// 不可达发布源：连不上是瞬间的事（本机回环的关闭端口），不必等超时
const UNREACHABLE: &str = "http://127.0.0.1:1";

/// 就绪的 hub：走对外入口 init，状态文件随后按用例预写
fn hub_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    hub::init(dir.path(), false).unwrap();
    dir
}

/// 写一份新鲜的缓存状态：24 小时节流下直接用缓存，不发请求
fn write_fresh_state(dir: &tempfile::TempDir, latest: Option<&str>) {
    let paths = HubPaths::from_root(dir.path());
    UpdateState {
        last_check: hub::unix_now(),
        latest: latest.map(str::to_string),
    }
    .save(&paths);
}

/// 把 `[update]` 表写进 hub.toml；只写想改的键，其余吃缺省值
fn write_update_config(dir: &tempfile::TempDir, body: &str) {
    let paths = HubPaths::from_root(dir.path());
    let mut config = hub::HubConfig::v1();
    config.update = toml::from_str::<hub::UpdateConfig>(body).unwrap();
    fs::write(paths.hub_toml(), config.to_toml().unwrap()).unwrap();
}

/// 缺省 gate：人读模式、连着终端、不在 CI
fn tty_gate() -> Gate {
    Gate {
        json: false,
        stderr_tty: true,
        in_ci: false,
    }
}

/// 一个恒高于当前版本的版本串：测试不受升版影响，永远构成「有新版本」
const NEWER: &str = "9.9.9";

fn notice(dir: &tempfile::TempDir, gate: &Gate) -> Option<String> {
    passive_notice(dir.path(), "list", 0, gate)
}

#[test]
fn fresh_cache_with_a_newer_version_gives_the_exact_line() {
    let dir = hub_dir();
    write_fresh_state(&dir, Some(NEWER));
    assert_eq!(
        notice(&dir, &tty_gate()).unwrap(),
        format!(
            "提示: fuyao-skills-hub 有新版本 {NEWER}（当前 {CURRENT_VERSION}），升级: npm i -g fuyao-skills-hub@latest"
        )
    );
}

#[test]
fn fresh_cache_not_newer_stays_silent() {
    let dir = hub_dir();
    write_fresh_state(&dir, Some(CURRENT_VERSION));
    assert_eq!(notice(&dir, &tty_gate()), None);
    write_fresh_state(&dir, Some("0.0.1"));
    assert_eq!(notice(&dir, &tty_gate()), None, "更旧的版本不该劝升级");
}

#[test]
fn no_cached_version_stays_silent() {
    let dir = hub_dir();
    write_fresh_state(&dir, None);
    assert_eq!(notice(&dir, &tty_gate()), None);
}

#[test]
fn mode_off_neither_notifies_nor_touches_the_state_file() {
    let dir = hub_dir();
    write_update_config(&dir, "mode = \"off\"\ninterval_hours = 0\n");
    write_fresh_state(&dir, Some("9.9.9"));
    assert_eq!(notice(&dir, &tty_gate()), None);
    // 状态没被改写 = 一次网络都没发（间隔为 0，只要查了就一定写 last_check）
    let paths = HubPaths::from_root(dir.path());
    assert!(UpdateState::load(&paths).latest.is_some());
}

#[test]
fn mode_manual_stays_silent_on_the_passive_path() {
    let dir = hub_dir();
    write_update_config(&dir, "mode = \"manual\"\ninterval_hours = 0\n");
    write_fresh_state(&dir, Some("9.9.9"));
    assert_eq!(notice(&dir, &tty_gate()), None);
}

#[test]
fn json_keeps_the_envelope_clean() {
    let dir = hub_dir();
    write_fresh_state(&dir, Some("9.9.9"));
    let gate = Gate {
        json: true,
        ..tty_gate()
    };
    assert_eq!(notice(&dir, &gate), None);
}

#[test]
fn a_failing_command_gets_no_notice() {
    let dir = hub_dir();
    write_fresh_state(&dir, Some("9.9.9"));
    assert_eq!(passive_notice(dir.path(), "list", 1, &tty_gate()), None);
}

#[test]
fn a_self_reporting_command_gets_no_notice() {
    let dir = hub_dir();
    write_fresh_state(&dir, Some("9.9.9"));
    // doctor 自己就把版本结论报告过一遍，不再往 stderr 补第二句
    assert_eq!(passive_notice(dir.path(), "doctor", 0, &tty_gate()), None);
}

#[test]
fn an_uninitialized_hub_gets_no_notice() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(notice(&dir, &tty_gate()), None);
}

#[test]
fn piped_stderr_is_silent_unless_opted_in() {
    let dir = hub_dir();
    write_fresh_state(&dir, Some("9.9.9"));
    let piped = Gate {
        stderr_tty: false,
        ..tty_gate()
    };
    assert_eq!(notice(&dir, &piped), None);

    write_update_config(&dir, "notify_when_piped = true\n");
    write_fresh_state(&dir, Some("9.9.9"));
    assert!(notice(&dir, &piped).is_some(), "显式打开后重定向也提示");
}

#[test]
fn ci_is_silent_unless_opted_in() {
    let dir = hub_dir();
    write_fresh_state(&dir, Some("9.9.9"));
    let ci = Gate {
        in_ci: true,
        ..tty_gate()
    };
    assert_eq!(notice(&dir, &ci), None);

    write_update_config(&dir, "notify_in_ci = true\n");
    write_fresh_state(&dir, Some("9.9.9"));
    assert!(notice(&dir, &ci).is_some(), "显式打开后 CI 里也提示");
}

#[test]
fn fresh_cache_sends_no_request_even_to_an_unreachable_source() {
    let dir = hub_dir();
    write_update_config(&dir, &format!("registry = \"{UNREACHABLE}\"\n"));
    write_fresh_state(&dir, Some(NEWER));
    let paths = HubPaths::from_root(dir.path());
    let before = UpdateState::load(&paths).last_check;
    // 缓存新鲜：直接用缓存，不发请求，因此这次检查时间原样不动
    assert!(notice(&dir, &tty_gate()).is_some());
    assert_eq!(UpdateState::load(&paths).last_check, before);
}

#[test]
fn an_unreachable_source_fails_silently_but_still_records_the_attempt() {
    let dir = hub_dir();
    write_update_config(
        &dir,
        &format!("registry = \"{UNREACHABLE}\"\ninterval_hours = 24\n"),
    );
    let paths = HubPaths::from_root(dir.path());
    // 从未查过 → 节流到期，这次会真的去查
    assert_eq!(notice(&dir, &tty_gate()), None, "连不上就静默放弃");
    let after = UpdateState::load(&paths);
    assert!(after.last_check > 0, "失败的检查也要记时间");
    assert_eq!(after.latest, None);
    // 刚记过时间 → 下一条命令不再等一次超时
    assert!(after.is_fresh(hub::unix_now(), 24));
}

#[test]
fn a_failed_check_does_not_overwrite_the_cached_version() {
    let dir = hub_dir();
    write_update_config(
        &dir,
        &format!("registry = \"{UNREACHABLE}\"\ninterval_hours = 24\n"),
    );
    let paths = HubPaths::from_root(dir.path());
    // 缓存里的高版本在场但已过期：这次检查会失败
    UpdateState {
        last_check: 0,
        latest: Some(NEWER.to_string()),
    }
    .save(&paths);
    // 查不到不等于缓存失效：上次查到的真实结论照常留着
    assert!(notice(&dir, &tty_gate()).is_some());
    assert_eq!(
        UpdateState::load(&paths).latest.as_deref(),
        Some(NEWER),
        "失败不得把已有缓存抹掉"
    );
}

#[test]
fn broken_config_falls_back_to_defaults() {
    let dir = hub_dir();
    let paths = HubPaths::from_root(dir.path());
    fs::write(paths.hub_toml(), "schema_version = \n").unwrap();
    assert_eq!(load_update_config(&paths), UpdateConfig::default());
    // 缺省档位是 auto：功能照常工作，只是配置取值回退了
    write_fresh_state(&dir, Some("9.9.9"));
    assert!(notice(&dir, &tty_gate()).is_some());
}

#[test]
fn a_successful_check_that_is_not_newer_still_refreshes_the_cache() {
    let dir = hub_dir();
    let paths = HubPaths::from_root(dir.path());
    let cfg = load_update_config(&paths);
    // 缓存里留着上次查到的高版本，但节流已过期
    UpdateState {
        last_check: 0,
        latest: Some(NEWER.to_string()),
    }
    .save(&paths);

    // 这次发布源回答的是与当前同版（比如刚回滚了发版）
    let latest = cached_or_refresh_with(&paths, &cfg, || Ok(CURRENT_VERSION.to_string()));
    assert_eq!(
        UpdateState::load(&paths).latest.as_deref(),
        Some(CURRENT_VERSION),
        "成功的检查无条件覆盖缓存：发布源说什么就是什么"
    );
    assert_eq!(
        latest.as_deref(),
        Some(CURRENT_VERSION),
        "刷新后缓存就是发布源说的那个版本"
    );
    assert!(
        UpdateState::load(&paths).last_check > 0,
        "成功的检查也要记时间"
    );
}

#[test]
fn a_successful_check_with_a_lower_version_downgrades_the_cache() {
    let dir = hub_dir();
    let paths = HubPaths::from_root(dir.path());
    let cfg = load_update_config(&paths);
    // 缓存里是一个高版本，但这个版本被撤回了，registry 回到 0.0.1
    UpdateState {
        last_check: 0,
        latest: Some(NEWER.to_string()),
    }
    .save(&paths);

    let latest = cached_or_refresh_with(&paths, &cfg, || Ok("0.0.1".to_string()));
    assert_eq!(
        UpdateState::load(&paths).latest.as_deref(),
        Some("0.0.1"),
        "缓存必须跟着降：不能永远告知一个已经不存在的版本"
    );
    assert_eq!(latest.as_deref(), Some("0.0.1"), "刷新后缓存跟着降");
}

#[test]
fn a_newer_answer_refreshes_the_cache() {
    let dir = hub_dir();
    let paths = HubPaths::from_root(dir.path());
    let cfg = load_update_config(&paths);

    let latest = cached_or_refresh_with(&paths, &cfg, || Ok("0.2.0".to_string()));
    assert_eq!(latest.as_deref(), Some("0.2.0"));
    assert_eq!(UpdateState::load(&paths).latest.as_deref(), Some("0.2.0"));
}

#[test]
fn an_unparsable_answer_is_skipped_without_touching_the_cache() {
    let dir = hub_dir();
    let paths = HubPaths::from_root(dir.path());
    let cfg = load_update_config(&paths);

    let latest = cached_or_refresh_with(&paths, &cfg, || Ok("not-a-version".to_string()));
    assert_eq!(latest, None);
    assert_eq!(UpdateState::load(&paths).latest, None);
    assert!(
        UpdateState::load(&paths).last_check > 0,
        "解析失败也算查过了"
    );
}

#[test]
fn missing_update_table_uses_the_six_defaults() {
    let dir = hub_dir();
    let paths = HubPaths::from_root(dir.path());
    // init 写的 hub.toml 只有 schema_version，没有 [update] 段
    let cfg = load_update_config(&paths);
    assert_eq!(cfg.mode, hub::UpdateMode::Auto);
    assert_eq!(cfg.interval_hours, 24);
    assert_eq!(cfg.registry, "https://registry.npmjs.org");
    assert_eq!(cfg.channel, "latest");
    assert!(!cfg.notify_in_ci);
    assert!(!cfg.notify_when_piped);
}

#[test]
fn update_state_file_is_not_tracked_by_git() {
    let dir = hub_dir();
    let paths = HubPaths::from_root(dir.path());
    UpdateState {
        last_check: hub::unix_now(),
        latest: Some(NEWER.to_string()),
    }
    .save(&paths);
    let repo = crate::git::open_hub(dir.path()).unwrap();
    assert!(
        !crate::git::is_dirty(&repo).unwrap(),
        "状态文件必须被 .gitignore 的 /* 挡在仓库外"
    );
    assert!(Path::new(&paths.update_toml()).is_file());
}
