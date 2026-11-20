//! 只读诊断：十一项检查、信封，以及持锁时仍给出真实可回收量。
//! 唯一联网的版本检查项用本地明文监听验成功路径，其余用例一律关掉它。

mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

use common::{
    Run, data_of, env_with_source, hub_root, init_hub_at, json, text_env, write_fresh_state,
    write_update_config,
};
use serde_json::Value;

/// 当前程序版本：结论要真的点名它，断言与程序同源
const CURRENT: &str = fsh_core::update::CURRENT_VERSION;

/// 起一个已 init 的 hub 根
fn hub() -> (tempfile::TempDir, String) {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    (install, hub)
}

/// 按检查项名取一条结论
fn check_of<'a>(data: &'a Value, name: &str) -> &'a Value {
    data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("缺少检查项: {name}"))
}

/// 起一个只应一次请求的本地发布源，返回它的根地址。
/// 明文 http、不碰 TLS：把 TLS 那一层留给 ureq 自己，测的是
/// 「请求 → 解析 → 比较 → 结论」整条链路
fn dist_tags_server(body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        // 先读一小段请求，保证客户端把请求发完再回话（内容不看）
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    format!("http://{addr}")
}

#[test]
fn doctor_reports_checks_and_envelope() {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    // 版本检查看网络：这一条用例只关心信封与本地各项，关掉它保持离线
    write_update_config(&hub, "mode = \"off\"\n");
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["doctor"]);
    // 临时目录环境：十一项检查、无 FAIL → 退出码 0
    assert_eq!(code, 0, "doctor: {out}");
    let d = data_of(&out);
    assert_eq!(d["checks"].as_array().unwrap().len(), 11);
    assert_eq!(d["fail"], serde_json::json!(false));
    // 两条内置检查都在：内容随 init 落地是 ok、未铺全局按设计是 warn，
    // 而 WARN 不改退出码
    let status = |name: &str| check_of(&d, name)["status"].as_str().unwrap().to_string();
    assert_eq!(status("builtin_global"), "warn");
    assert_eq!(status("builtin_content"), "ok");
    assert_eq!(code, 0, "WARN 不改退出码: {out}");
}

/// 有写操作进行时（全局写锁被占），只读诊断仍要跑完十一项
#[test]
fn doctor_runs_while_a_writer_holds_the_lock() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let src_s = src.display().to_string();
    json(&hub, &["add", &src_s]);
    write_update_config(&hub, "mode = \"off\"\n");

    // 本进程持住全局写锁，模拟「有人正在写」
    let paths = fsh_core::paths::HubPaths::from_root(Path::new(&hub));
    let _held = fsh_core::lock::write_lock(&paths).expect("测试进程应能拿到锁");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["doctor"]);
    assert_eq!(code, 0, "持锁时 doctor 仍应跑完: {out}");
    let d = data_of(&out);
    let checks = d["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 11, "十一项都要在: {out}");
    assert!(
        checks.iter().any(|c| c["name"] == "orphan_destinations"),
        "要有 orphan_destinations 项"
    );
}

/// 发布源上有更新版本：warn 并点名当前版本、最新版本与升级命令，退出码不变。
/// 缓存刚查过也照样真查——doctor 问的是「现在」，节流在这里不生效
#[test]
fn a_newer_published_version_warns_without_failing_the_run() {
    let (_install, hub) = hub();
    let registry = dist_tags_server(r#"{"latest":"99.0.0"}"#);
    write_update_config(&hub, &format!("registry = \"{registry}\"\n"));
    write_fresh_state(&hub, Some("0.0.1"));

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["doctor"]);
    assert_eq!(code, 0, "上游发版不该让脚本变红: {out}");
    let d = data_of(&out);
    assert_eq!(d["fail"], serde_json::json!(false));
    let item = check_of(&d, "update_available");
    assert_eq!(item["status"], "warn", "{item}");
    let detail = item["detail"].as_str().unwrap();
    assert!(
        detail.contains("99.0.0") && detail.contains(CURRENT),
        "点名当前版本与最新版本: {detail}"
    );
    assert!(
        detail.contains("npm i -g fuyao-skills-hub@latest"),
        "给可照抄的升级命令: {detail}"
    );
}

/// 发布源上不比当前新的版本：给通过
#[test]
fn a_published_version_that_is_not_newer_passes() {
    let (_install, hub) = hub();
    let registry = dist_tags_server(r#"{"latest":"0.0.1"}"#);
    write_update_config(&hub, &format!("registry = \"{registry}\"\n"));

    let Run { stdout: out, .. } = json(&hub, &["doctor"]);
    let d = data_of(&out);
    let item = check_of(&d, "update_available");
    assert_eq!(item["status"], "ok", "{out}");
    let detail = item["detail"].as_str().unwrap();
    assert!(
        detail.contains("已是最新") && detail.contains(CURRENT),
        "点名当前版本: {detail}"
    );
}

/// 发布源返回的版本串读不懂：核对不了，告警并点名原因，不盖章成通过
#[test]
fn an_unparsable_published_version_warns() {
    let (_install, hub) = hub();
    let registry = dist_tags_server(r#"{"latest":"not-a-version"}"#);
    write_update_config(&hub, &format!("registry = \"{registry}\"\n"));

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["doctor"]);
    assert_eq!(code, 0, "读不懂不是 FAIL: {out}");
    let d = data_of(&out);
    let item = check_of(&d, "update_available");
    assert_eq!(item["status"], "warn", "核对不了不等于通过: {item}");
    let detail = item["detail"].as_str().unwrap();
    assert!(
        detail.contains("无法核对版本") && detail.contains("not-a-version"),
        "点名解析不了的那个串: {detail}"
    );
}

/// manual 的语义就是「只有 doctor 查」：这一档照样真的去查
#[test]
fn manual_mode_still_queries_from_doctor() {
    let (_install, hub) = hub();
    let registry = dist_tags_server(r#"{"latest":"99.0.0"}"#);
    write_update_config(
        &hub,
        &format!("mode = \"manual\"\nregistry = \"{registry}\"\n"),
    );

    let Run { stdout: out, .. } = json(&hub, &["doctor"]);
    let d = data_of(&out);
    assert_eq!(check_of(&d, "update_available")["status"], "warn", "{out}");
}

/// 发布源不可达：warn 并点名原因，不盖章成通过，其余各项照常
#[test]
fn an_unreachable_registry_warns_with_the_reason() {
    let (_install, hub) = hub();
    write_update_config(&hub, "registry = \"http://127.0.0.1:1\"\n");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["doctor"]);
    assert_eq!(code, 0, "查不到不是 FAIL: {out}");
    let d = data_of(&out);
    let item = check_of(&d, "update_available");
    assert_eq!(item["status"], "warn", "核对不了不等于通过: {item}");
    let detail = item["detail"].as_str().unwrap();
    assert!(detail.contains("无法核对版本"), "{detail}");
    assert!(detail.contains("127.0.0.1"), "点名原因: {detail}");
}

/// 明确关掉版本检查：给通过并说明关着，用户的决定不该被反复唠叨
#[test]
fn a_disabled_check_reports_ok() {
    let (_install, hub) = hub();
    // 发布源指向不可达地址：真去查了就会变 warn，通过本身证明没查
    write_update_config(&hub, "mode = \"off\"\nregistry = \"http://127.0.0.1:1\"\n");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["doctor"]);
    assert_eq!(code, 0, "{out}");
    let d = data_of(&out);
    let item = check_of(&d, "update_available");
    assert_eq!(item["status"], "ok", "{item}");
    assert!(
        item["detail"].as_str().unwrap().contains("版本检查已关闭"),
        "说明检查关着: {item}"
    );
}

/// doctor 自己就是报告：被动升级提示不该在它跑完后再补一行到 stderr
#[test]
fn doctor_does_not_repeat_the_notice_on_stderr() {
    let (_install, hub) = hub();
    // 被动路径的条件全部打开：缓存里有个更高版本，管道里也允许提示
    write_update_config(
        &hub,
        "notify_when_piped = true\nregistry = \"http://127.0.0.1:1\"\n",
    );
    write_fresh_state(&hub, Some("9.9.9"));

    let Run {
        stdout: out,
        stderr,
        code,
    } = text_env(&[("CI", "")], &hub, &["doctor"]);
    assert_eq!(code, 0, "doctor: {out}");
    assert!(out.contains("update_available"), "报告里说一次: {out}");
    assert!(
        !stderr.contains("提示:"),
        "doctor 不该额外补一行升级提示: {stderr}"
    );
}
