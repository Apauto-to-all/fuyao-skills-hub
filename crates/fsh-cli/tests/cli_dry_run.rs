//! 预演：动作清单与真跑效果一致，且零落盘。

mod common;

use common::{data_of, env_with_source, fsh, hub_root};

/// `actions` 里是否含一步：expected 里点名的字段全部对上才算
fn has_step(actions: &serde_json::Value, expected: serde_json::Value) -> bool {
    let expected = expected.as_object().unwrap().clone();
    actions.as_array().unwrap().iter().any(|step| {
        expected
            .iter()
            .all(|(field, value)| step.get(field) == Some(value))
    })
}

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
    let actions = &d["actions"];
    assert!(
        has_step(
            actions,
            serde_json::json!({"action": "copy", "from": src_s, "to": "store/demo"})
        ),
        "add dry 应列出复制一步: {actions}"
    );
    assert!(
        has_step(actions, serde_json::json!({"action": "commit"})),
        "add dry 应列出提交一步: {actions}"
    );
    assert!(
        !install.path().join("store/demo").exists(),
        "预演不得把 demo 复制进 store"
    );

    // archive / remove --dry-run：动作清单各就各位，盘上一字未动
    let (_, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
    assert_eq!(code, 0);
    let (out, code) = fsh(&["--json", "--hub", &hub, "archive", "demo", "--dry-run"]);
    assert_eq!(code, 0, "archive dry: {out}");
    let actions = &data_of(&out)["actions"];
    assert!(
        has_step(
            actions,
            serde_json::json!({"action": "move", "from": "store/demo", "to": "archive/demo"})
        ),
        "archive dry 应列出移动一步: {actions}"
    );
    let (out, code) = fsh(&["--json", "--hub", &hub, "remove", "demo", "--dry-run"]);
    assert_eq!(code, 0, "remove dry: {out}");
    let actions = &data_of(&out)["actions"];
    assert!(
        has_step(
            actions,
            serde_json::json!({"action": "remove", "target": "store/demo"})
        ),
        "remove dry 应列出删工作副本: {actions}"
    );
    assert!(
        has_step(actions, serde_json::json!({"action": "commit"})),
        "remove dry 应列出提交一步: {actions}"
    );
    assert!(install.path().join("store/demo/SKILL.md").is_file());

    // 预演列的动作要跟真跑的实际效果逐条对上：清单不是另一套描述
    let (_, code) = fsh(&["--json", "--hub", &hub, "remove", "demo"]);
    assert_eq!(code, 0);
    assert!(
        !install.path().join("store/demo").exists(),
        "预演列了 remove store/demo，真跑就该删掉它"
    );
    let (listed, code) = fsh(&["--json", "--hub", &hub, "list", "demo"]);
    assert_eq!(code, 0, "真跑后列表应没有 demo: {listed}");
    assert!(
        !data_of(&listed)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == "demo")
    );

    // restore / init --dry-run 语义存在且退出码 0
    let (_, code) = fsh(&["--json", "--hub", &hub, "restore", "demo", "--dry-run"]);
    assert_eq!(code, 1, "未归档键的恢复预演按不存在拒绝");
    let (_, code) = fsh(&["--json", "--hub", &hub, "init", "--dry-run"]);
    assert_eq!(code, 0);
}
