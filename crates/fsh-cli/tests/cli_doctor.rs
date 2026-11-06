//! 只读诊断：八项检查、信封，以及持锁时仍给出真实可回收量。

mod common;

use std::path::Path;

use common::{data_dir, data_of, env_with_source, fsh};
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

/// 有写操作进行时（全局写锁被占），只读诊断仍要跑完八项，
/// 且可回收量要是真实值——不能因为拿不到锁就印 0 装绿
#[test]
fn doctor_reports_real_reclaimable_while_a_writer_holds_the_lock() {
    let (install, src) = env_with_source();
    let hub = data_dir(install.path());
    fsh(&["--json", "--hub", &hub, "init"]);
    let src_s = src.display().to_string();
    fsh(&["--json", "--hub", &hub, "add", &src_s]);

    // 造一个没人引用的导出目录：gc 预演算得出可回收空间
    let orphan = install.path().join("data/export/demo/9.9.9");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join("SKILL.md"), "orphan").unwrap();

    // 本进程持住全局写锁，模拟「有人正在写」
    let paths = fsh_core::paths::HubPaths::from_root(Path::new(&hub));
    let _held = fsh_core::lock::write_lock(&paths).expect("测试进程应能拿到锁");

    let (out, code) = fsh(&["--json", "--hub", &hub, "doctor"]);
    assert_eq!(code, 0, "持锁时 doctor 仍应跑完: {out}");
    let d = data_of(&out);
    let checks = d["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 8, "八项都要在: {out}");
    let detail = checks
        .iter()
        .find(|c| c["name"] == "reclaimable")
        .expect("要有 reclaimable 项")["detail"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        !detail.contains("可回收 0 字节") && !detail.contains("统计失败"),
        "持锁时也该算出真实可回收量，不能假绿: {detail}"
    );
}
