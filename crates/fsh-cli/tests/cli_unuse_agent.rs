//! 按 agent 撤回：真实二进制上的落点取舍、名单校验与预演。

mod common;

use common::{Run, data_of, env_with_source, error_of, hub_with_used_skill, initialized_hub, json};

/// 按 agent 撤回（共用落点）：留一个引用者时目录不删、记录只摘名字，
/// 最后一个引用者也撤走时目录与记录同删；两种 --agent 写法等价，预演只列将删的落点。
#[test]
fn unuse_by_agent_keeps_a_shared_destination_until_the_last_agent_leaves() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let dest = project.path().join(".agents/skills/demo");

    // universal 与 kimi-code-cli 的项目技能目录都是 .agents/skills：
    // 重复给值铺一次，两方共用一处落点
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "use",
            "--skill",
            "demo",
            "--agent",
            "universal",
            "--agent",
            "kimi-code-cli",
            "--project",
            &proj,
        ],
    );
    assert_eq!(code, 0, "use 重复给值: {out}");
    assert_eq!(data_of(&out)["applied"], serde_json::json!(["demo"]));
    assert!(dest.is_dir());

    // 预演另一个项目：两个 agent 共用一处落点，只列一步建链接
    let dry_project = tempfile::tempdir().unwrap();
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "use",
            "--skill",
            "demo",
            "--agent",
            "universal",
            "--agent",
            "kimi-code-cli",
            "--project",
            &dry_project.path().display().to_string(),
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "use 预演: {out}");
    let links = data_of(&out)["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| action["action"] == serde_json::json!("link"))
        .count();
    assert_eq!(links, 1, "两个 agent 只建一处落点: {out}");
    assert!(!dry_project.path().join(".agents").exists(), "预演不得落盘");

    // 逗号与重复给值等价：预演载荷逐字相同，盘上零改动
    let comma = json(
        &hub,
        &[
            "unuse",
            "demo",
            "--agent",
            "universal,kimi-code-cli",
            "--project",
            &proj,
            "--dry-run",
        ],
    );
    let repeat = json(
        &hub,
        &[
            "unuse",
            "demo",
            "--agent",
            "universal",
            "--agent",
            "kimi-code-cli",
            "--project",
            &proj,
            "--dry-run",
        ],
    );
    assert_eq!(comma.code, 0, "逗号写法: {}", comma.stdout);
    assert_eq!(repeat.code, 0, "重复写法: {}", repeat.stdout);
    assert_eq!(
        data_of(&comma.stdout),
        data_of(&repeat.stdout),
        "两种写法必须等价"
    );
    assert_eq!(
        data_of(&comma.stdout)["targets"][0]["actions"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "两个 agent 都撤才轮到共用落点: {}",
        comma.stdout
    );
    assert!(dest.is_dir(), "预演不得落盘");

    // 只撤 universal 的预演：共用目录还有人用，不列任何将删的落点
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "unuse",
            "demo",
            "--agent",
            "universal",
            "--project",
            &proj,
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "按 agent 撤回预演: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    assert!(
        d["targets"][0].get("actions").is_none(),
        "共用目录留着给 kimi-code-cli，预演不得虚报要删: {d}"
    );

    // 实跑只撤 universal：目录留给 kimi-code-cli，记录里只摘掉被撤的
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["unuse", "demo", "--agent", "universal", "--project", &proj],
    );
    assert_eq!(code, 0, "按 agent 撤回: {out}");
    let target = &data_of(&out)["targets"][0];
    assert_eq!(target["agents"], serde_json::json!(["universal"]));
    assert!(
        target.get("removed").is_none(),
        "共用落点这次没删，removed 不得虚报: {target}"
    );
    assert!(target.get("link").is_some(), "撤回报告带出链接方式");
    assert!(dest.is_dir(), "kimi-code-cli 还在用，共用目录不得删");
    let registry = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert!(registry.contains("kimi-code-cli"), "{registry}");
    assert!(
        !registry.contains("universal"),
        "被撤的 agent 须摘掉: {registry}"
    );

    // 最后一个引用者也撤走：落点与整条记录一起清掉
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "unuse",
            "demo",
            "--agent",
            "kimi-code-cli",
            "--project",
            &proj,
        ],
    );
    assert_eq!(code, 0, "撤最后一个引用者: {out}");
    let target = &data_of(&out)["targets"][0];
    assert_eq!(target["agents"], serde_json::json!(["kimi-code-cli"]));
    assert_eq!(target["removed"].as_array().unwrap().len(), 1);
    assert!(!dest.exists(), "没有引用者了，落点必须删");
    let registry =
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap_or_default();
    assert!(!registry.contains("[[projects."), "{registry}");
}

/// 按 agent 撤回（各 agent 各自目录）：removed 就是真删的那处落点，另一处原样留着；
/// 名单点满剩下那一处时落点与记录一起清掉。
#[test]
fn unuse_by_agent_reports_the_destination_it_really_removed() {
    let used = hub_with_used_skill();
    let kept = &used.dest_dsh;
    let doomed = &used.dest_agents;
    assert!(kept.is_dir() && doomed.is_dir());

    // 只撤 universal：删掉它自己的落点，dsh 那处不动，记录只剩 dsh
    let Run {
        stdout: out, code, ..
    } = json(
        &used.hub,
        &[
            "unuse",
            "demo",
            "--agent",
            "universal",
            "--project",
            &used.proj,
        ],
    );
    assert_eq!(code, 0, "按 agent 撤回: {out}");
    let target = &data_of(&out)["targets"][0];
    assert_eq!(target["agents"], serde_json::json!(["universal"]));
    let removed = target["removed"].as_array().unwrap();
    assert_eq!(removed.len(), 1, "只报这次真删的落点: {target}");
    assert!(
        std::path::Path::new(removed[0].as_str().unwrap()).ends_with(".agents/skills/demo"),
        "{target}"
    );
    assert!(!doomed.exists(), "被撤 agent 的落点必须删");
    assert!(kept.is_dir(), "没被撤的 agent 落点不得动");
    let registry = std::fs::read_to_string(used.install.path().join("projects.toml")).unwrap();
    assert!(registry.contains("dsh"), "{registry}");
    assert!(!registry.contains("universal"), "{registry}");

    // 名单点满剩下的那一处：落点与记录一起清掉
    let Run {
        stdout: out, code, ..
    } = json(
        &used.hub,
        &["unuse", "demo", "--agent", "dsh", "--project", &used.proj],
    );
    assert_eq!(code, 0, "整条撤回: {out}");
    assert_eq!(
        data_of(&out)["targets"][0]["agents"],
        serde_json::json!(["dsh"])
    );
    assert!(!kept.exists());
}

/// 名单点到记录里没有的 agent：稳定码 agent_not_registered，盘与登记簿不动。
#[test]
fn unuse_by_agent_rejects_an_agent_the_record_does_not_have() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let dest = project.path().join(".agents/skills/demo");
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "--skill",
                "demo",
                "--agent",
                "universal",
                "--project",
                &proj
            ]
        )
        .code,
        0
    );
    let before = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["unuse", "demo", "--agent", "dsh", "--project", &proj],
    );
    assert_eq!(code, 1, "{out}");
    let err = error_of(&out);
    assert_eq!(
        err["code"],
        serde_json::json!("agent_not_registered"),
        "{err}"
    );
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("dsh"), "点名写错的 agent: {message}");
    assert!(
        message.contains("universal"),
        "点名记录里已有的 agent: {message}"
    );
    assert!(dest.is_dir(), "不合格的名单不得动盘");
    assert_eq!(
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap(),
        before,
        "登记簿一字不动"
    );
}
