//! 版本切换：真实二进制上的换靶、幂等命中、预演与失败码。

mod common;

use common::{data_of, env_with_versioned_skill, error_of, fsh, hub_root};

/// 把 demo 铺到一个新项目里（跟随最新），返回 use 的 JSON 信封原文
fn use_into(hub: &str, proj: &str) -> String {
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        hub,
        "use",
        "demo",
        "--agent",
        "universal,dsh",
        "--project",
        proj,
    ]);
    assert_eq!(code, 0, "use: {out}");
    out
}

/// 登记簿里这条记录的版本（null = 跟最新）
fn registered_version(hub: &str) -> serde_json::Value {
    let (out, code) = fsh(&["--json", "--hub", hub, "show", "demo"]);
    assert_eq!(code, 0, "show: {out}");
    data_of(&out)["usage"][0]["version"].clone()
}

#[test]
fn upgrade_switches_a_registered_record_through_real_binary() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    use_into(&hub, &proj);
    assert_eq!(registered_version(&hub), serde_json::json!(null));

    // 换到固定版本：记录跟着换，落点仍可读
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@1.0.0",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "upgrade: {out}");
    let d = data_of(&out);
    assert_eq!(d["version"], serde_json::json!("1.0.0"));
    let target = &d["targets"][0];
    assert_eq!(target["from"], serde_json::json!(null), "原来是跟最新");
    assert_eq!(target["scope"], serde_json::json!("project"));
    assert_eq!(target["agents"], serde_json::json!(["universal", "dsh"]));
    assert_eq!(target["destinations"].as_array().unwrap().len(), 2);
    assert!(target.get("link").is_some(), "实跑报告带出实际档位");
    assert_eq!(registered_version(&hub), serde_json::json!("1.0.0"));
    for name in [".agents/skills/demo", ".dsh/skills/demo"] {
        assert!(
            project.path().join(name).join("SKILL.md").is_file(),
            "{name} 换靶后仍可读"
        );
    }
    // 同一版本再换一次：幂等命中，零盘动作
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@1.0.0",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "幂等命中不该报错: {out}");
    let d = data_of(&out);
    assert_eq!(d["targets"][0]["idempotent"], serde_json::json!(true));
    assert!(
        d["targets"][0].get("note").is_none(),
        "链接档跟最新没有拷贝提醒: {d}"
    );
    // 幂等命中的预演同样只说明没有盘动作
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@1.0.0",
        "--project",
        &proj,
        "--dry-run",
    ]);
    assert_eq!(code, 0, "幂等命中的预演: {out}");
    let d = data_of(&out);
    assert_eq!(d["targets"][0]["idempotent"], serde_json::json!(true));
    let actions = d["targets"][0]["actions"].as_array().unwrap();
    assert!(
        actions
            .iter()
            .all(|a| a.as_str().unwrap_or_default().contains("无盘动作")),
        "幂等命中的预演只说明没有盘动作: {actions:?}"
    );

    // 不给版本 = 跟随最新：记录版本置空
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "upgrade 跟最新: {out}");
    let d = data_of(&out);
    assert_eq!(d["version"], serde_json::json!(null));
    assert_eq!(d["targets"][0]["from"], serde_json::json!("1.0.0"));
    assert_eq!(registered_version(&hub), serde_json::json!(null));
}

#[test]
fn upgrade_dry_run_lists_the_swap_and_touches_nothing() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    use_into(&hub, &proj);

    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@1.0.0",
        "--project",
        &proj,
        "--dry-run",
    ]);
    assert_eq!(code, 0, "upgrade --dry-run: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    let actions = d["targets"][0]["actions"].as_array().unwrap();
    let joined = actions
        .iter()
        .map(|a| a.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("跟最新") && joined.contains("@1.0.0"),
        "预演要打出旧版本 → 新版本: {joined}"
    );
    assert!(joined.contains("rename"), "预演要列出换靶三步: {joined}");
    assert!(joined.contains(".fsh-old"), "预演要列出占位名: {joined}");
    // 预演零落盘：登记版本没动，落点与导出目录都没变
    assert_eq!(registered_version(&hub), serde_json::json!(null));
    assert!(
        !install.path().join("export/demo/1.0.0").exists(),
        "预演不得导出"
    );
    assert!(
        project
            .path()
            .join(".agents/skills/demo/SKILL.md")
            .is_file()
    );
}

#[test]
fn upgrade_failure_codes_are_stable_over_real_binary() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );

    // 没有登记记录：版本切换只改已有记录
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@1.0.0",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("not_registered"));

    // 没有目标
    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade", "demo"]);
    assert_eq!(code, 1);
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("no_target"));
    assert!(
        err["message"].as_str().unwrap().contains("--project ."),
        "没有目标要点出显式 --project .: {err}"
    );

    // 项目目录不存在
    let missing = project.path().join("nope");
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo",
        "--project",
        &missing.display().to_string(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("invalid_project"));

    // 版本不存在 / 技能说明非法
    use_into(&hub, &proj);
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@9.9.9",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("version_not_found")
    );
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@1.0",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("invalid_skill_spec")
    );

    // 占位残留：点名完整路径、绝不代删
    let residue = project.path().join(".agents/skills/.demo.fsh-old");
    std::fs::create_dir_all(&residue).unwrap();
    std::fs::write(residue.join("left-over.txt"), "keep").unwrap();
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "demo@1.0.0",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("placeholder_residue"));
    assert!(
        err["message"].as_str().unwrap().contains(".fsh-old"),
        "报错要点名占位路径: {err}"
    );
    assert!(residue.join("left-over.txt").is_file(), "残骸绝不自动删");
    assert_eq!(registered_version(&hub), serde_json::json!(null));
}
