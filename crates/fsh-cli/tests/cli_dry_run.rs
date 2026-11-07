//! 预演：动作清单与真跑效果一致，且零落盘。

mod common;

use common::{data_of, env_with_source, fsh, hub_root};
#[test]
fn dry_runs_plan_without_touching_disk() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
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
            .join("store")
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
    let actions: Vec<String> = data_of(&out)["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    assert!(actions.iter().any(|a| a == "delete tag demo/v0.1.0"));
    assert!(install.path().join("store/demo/SKILL.md").is_file());

    // 预演列的动作要跟真跑的实际效果逐条对上：清单不是另一套描述
    let (_, code) = fsh(&["--json", "--hub", &hub, "remove", "demo"]);
    assert_eq!(code, 0);
    assert!(
        !install.path().join("store/demo").exists(),
        "预演列了 remove store/demo，真跑就该删掉它"
    );
    assert!(
        !install.path().join("export/demo").exists(),
        "预演列了 remove 导出目录，真跑就该删掉它"
    );
    let (listed, _) = fsh(&["--json", "--hub", &hub, "list"]);
    assert!(
        !listed.contains("demo"),
        "预演列了 delete tag，真跑后列表里不该还有 demo: {listed}"
    );

    // gc --dry-run / init --dry-run 语义存在且退出码 0
    let (_, code) = fsh(&["--json", "--hub", &hub, "gc", "--dry-run"]);
    assert_eq!(code, 0);
    let (_, code) = fsh(&["--json", "--hub", &hub, "init", "--dry-run"]);
    assert_eq!(code, 0);
}
