//! 预演：动作清单与真跑效果一致，且零落盘。

mod common;

use common::{Run, data_of, env_with_source, hub_root, init_hub_at, json};

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
    init_hub_at(&hub);
    let src_s = src.display().to_string();

    // add --dry-run：动作清单齐全，store 一字未动
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--dry-run"]);
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
    let Run { code, .. } = json(&hub, &["add", &src_s]);
    assert_eq!(code, 0);
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "demo", "--dry-run"]);
    assert_eq!(code, 0, "archive dry: {out}");
    let actions = &data_of(&out)["actions"];
    assert!(
        has_step(
            actions,
            serde_json::json!({"action": "move", "from": "store/demo", "to": "archive/demo"})
        ),
        "archive dry 应列出移动一步: {actions}"
    );
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "demo", "--dry-run"]);
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
    let Run { code, .. } = json(&hub, &["remove", "demo"]);
    assert_eq!(code, 0);
    assert!(
        !install.path().join("store/demo").exists(),
        "预演列了 remove store/demo，真跑就该删掉它"
    );
    let Run {
        stdout: listed,
        code,
        ..
    } = json(&hub, &["list", "demo"]);
    assert_eq!(code, 0, "真跑后列表应没有 demo: {listed}");
    assert!(
        !data_of(&listed)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["key"] == "demo")
    );

    // restore / init --dry-run 语义存在且退出码 0
    let Run { code, .. } = json(&hub, &["restore", "demo", "--dry-run"]);
    assert_eq!(code, 1, "未归档键的恢复预演按不存在拒绝");
    let Run { code, .. } = json(&hub, &["init", "--dry-run"]);
    assert_eq!(code, 0);
}
/// -n 是 --dry-run 的短名：init/add/archive/remove/restore 五条写命令上给出同一份动作清单
/// （use 与 unuse 的 -n 在 cli_aliases 的短名等价用例里覆盖）
#[test]
fn dry_run_short_flag_matches_long_flag_across_write_commands() {
    let (install, src) = env_with_source();
    let src_s = src.display().to_string();
    let hub = hub_root(install.path());

    // 预演不动盘，两种写法可以在同一个 hub 上先后跑
    assert_eq!(json(&hub, &["init"]).code, 0);
    for (cmd, args) in [("init", vec!["init"]), ("add", vec!["add", &src_s])] {
        let mut short = args.clone();
        short.push("-n");
        let mut long = args.clone();
        long.push("--dry-run");
        let Run {
            stdout: short_out,
            code,
            ..
        } = json(&hub, &short);
        assert_eq!(code, 0, "{cmd} -n: {short_out}");
        let Run {
            stdout: long_out, ..
        } = json(&hub, &long);
        assert_eq!(
            data_of(&short_out),
            data_of(&long_out),
            "{cmd} -n 与 --dry-run 的 data 应相同"
        );
    }

    // 真入库后 archive 与 remove 的预演各比一次
    assert_eq!(json(&hub, &["add", &src_s]).code, 0);
    for cmd in ["archive", "remove"] {
        let Run {
            stdout: short_out,
            code,
            ..
        } = json(&hub, &[cmd, "demo", "-n"]);
        assert_eq!(code, 0, "{cmd} -n: {short_out}");
        let Run {
            stdout: long_out, ..
        } = json(&hub, &[cmd, "demo", "--dry-run"]);
        assert_eq!(
            data_of(&short_out),
            data_of(&long_out),
            "{cmd} -n 与 --dry-run 的 data 应相同"
        );
    }

    // 归档后 restore 的预演
    assert_eq!(json(&hub, &["archive", "demo"]).code, 0);
    let Run {
        stdout: short_out,
        code,
        ..
    } = json(&hub, &["restore", "demo", "-n"]);
    assert_eq!(code, 0, "restore -n: {short_out}");
    let Run {
        stdout: long_out, ..
    } = json(&hub, &["restore", "demo", "--dry-run"]);
    assert_eq!(data_of(&short_out), data_of(&long_out));
}
