//! 只读诊断：九项检查、信封，以及持锁时仍给出真实可回收量。

mod common;

use std::path::Path;

use common::{data_of, env_with_source, fsh, hub_root};
#[test]
fn doctor_reports_checks_and_envelope() {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    let (out, code) = fsh(&["--json", "--hub", &hub, "doctor"]);
    // 临时目录环境：九项检查、无 FAIL → 退出码 0
    assert_eq!(code, 0, "doctor: {out}");
    let d = data_of(&out);
    assert_eq!(d["checks"].as_array().unwrap().len(), 9);
    assert_eq!(d["fail"], serde_json::json!(false));
    // 两条内置检查都在：内容随 init 落地是 ok、未铺全局按设计是 warn，
    // 而 WARN 不改退出码
    let checks = d["checks"].as_array().unwrap();
    let status = |name: &str| {
        checks
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("缺少检查项: {name}"))["status"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(status("builtin_global"), "warn");
    assert_eq!(status("builtin_content"), "ok");
    assert_eq!(code, 0, "WARN 不改退出码: {out}");
}

/// 有写操作进行时（全局写锁被占），只读诊断仍要跑完八项
#[test]
fn doctor_runs_while_a_writer_holds_the_lock() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    let src_s = src.display().to_string();
    fsh(&["--json", "--hub", &hub, "add", &src_s]);

    // 本进程持住全局写锁，模拟「有人正在写」
    let paths = fsh_core::paths::HubPaths::from_root(Path::new(&hub));
    let _held = fsh_core::lock::write_lock(&paths).expect("测试进程应能拿到锁");

    let (out, code) = fsh(&["--json", "--hub", &hub, "doctor"]);
    assert_eq!(code, 0, "持锁时 doctor 仍应跑完: {out}");
    let d = data_of(&out);
    let checks = d["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 9, "九项都要在: {out}");
    assert!(
        checks.iter().any(|c| c["name"] == "orphan_destinations"),
        "要有 orphan_destinations 项"
    );
}
