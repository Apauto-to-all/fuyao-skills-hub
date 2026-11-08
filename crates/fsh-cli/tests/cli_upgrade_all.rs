//! 批量升级：真实二进制上的落后判定、跳过清单、预演与失败即停的退出码。

mod common;

use common::{data_of, env_with_versioned_skill, error_of, fsh, hub_root};

/// 把 demo 再发一版：改写工作副本的 frontmatter 版本后走 release，truth 由 git 打 tag
fn release_as(install: &std::path::Path, hub: &str, version: &str) {
    std::fs::write(
        install.join("store/demo/SKILL.md"),
        format!(
            "---
name: demo
description: 演示
metadata:
  version: \"{version}\"
---

正文
"
        ),
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", hub, "release", "demo"]);
    assert_eq!(code, 0, "release: {out}");
}

/// 把 demo 按 spec 铺到一个项目里的某 agent
fn use_into(hub: &str, proj: &str, spec: &str) {
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        hub,
        "use",
        spec,
        "--agent",
        "universal",
        "--project",
        proj,
    ]);
    assert_eq!(code, 0, "use {spec}: {out}");
}

/// 登记簿里某个项目这条记录的版本（null = 跟最新）
fn registered_version(hub: &str, proj: &str) -> serde_json::Value {
    let (out, code) = fsh(&["--json", "--hub", hub, "show", "demo"]);
    assert_eq!(code, 0, "show: {out}");
    let usage = data_of(&out)["usage"].as_array().unwrap().clone();
    let name = std::path::Path::new(proj)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    usage
        .iter()
        .find(|u| u["project"].as_str().unwrap_or_default().ends_with(&name))
        .unwrap_or_else(|| panic!("登记簿里应有 {name} 的记录: {usage:?}"))["version"]
        .clone()
}

#[test]
fn upgrade_all_advances_stale_records_and_reports_skips() {
    let (install, src) = env_with_versioned_skill("0.1.0");
    let hub = hub_root(install.path());
    let stale = tempfile::tempdir().unwrap();
    let following = tempfile::tempdir().unwrap();
    let stale_dir = stale.path().display().to_string();
    let following_dir = following.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    use_into(&hub, &stale_dir, "demo@0.1.0");
    use_into(&hub, &following_dir, "demo");
    release_as(install.path(), &hub, "1.0.0");

    // 预演：给出「旧版本 → 最新」的动作清单，盘上一字未动
    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade", "--all", "--dry-run"]);
    assert_eq!(code, 0, "dry-run: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    assert_eq!(d["upgraded"].as_array().unwrap().len(), 1);
    let actions = d["upgraded"][0]["targets"][0]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        actions.contains("0.1.0") && actions.contains("@1.0.0"),
        "预演要打出旧版本 → 最新: {actions}"
    );
    assert!(
        !install.path().join("export/demo/1.0.0").exists(),
        "预演不得导出新版本"
    );
    assert_eq!(
        registered_version(&hub, &stale_dir),
        serde_json::json!("0.1.0"),
        "预演不写登记簿"
    );
    let skipped = d["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1, "跟最新的记录要逐条报出: {skipped:?}");
    assert!(
        skipped[0]["reason"].as_str().unwrap().contains("跟最新"),
        "跳过原因要说清: {}",
        skipped[0]["reason"]
    );

    // 范围收窄：只给 --project 就只扫那一个项目，没有落后记录即空清单
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "upgrade",
        "--all",
        "--project",
        &following_dir,
    ]);
    assert_eq!(code, 0, "收窄: {out}");
    assert!(
        data_of(&out).get("upgraded").is_none(),
        "跟最新的项目里没有可升记录"
    );

    // 实跑：记录升到最新 tag，落点仍可读
    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade", "--all"]);
    assert_eq!(code, 0, "upgrade --all: {out}");
    let d = data_of(&out);
    assert_eq!(d["upgraded"].as_array().unwrap().len(), 1);
    assert_eq!(d["upgraded"][0]["key"], serde_json::json!("demo"));
    assert_eq!(d["upgraded"][0]["version"], serde_json::json!("1.0.0"));
    assert_eq!(
        d["upgraded"][0]["targets"][0]["from"],
        serde_json::json!("0.1.0")
    );
    assert_eq!(
        registered_version(&hub, &stale_dir),
        serde_json::json!("1.0.0")
    );
    assert!(
        stale.path().join(".agents/skills/demo/SKILL.md").is_file(),
        "换靶后落点仍可读"
    );

    // 再跑一次：没有落后记录，零升级、零退出码
    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade", "--all"]);
    assert_eq!(code, 0, "幂等: {out}");
    assert!(
        data_of(&out).get("upgraded").is_none(),
        "没有落后记录: {out}"
    );
}

#[test]
fn upgrade_all_stops_on_failure_and_exits_one() {
    let (install, src) = env_with_versioned_skill("0.1.0");
    let hub = hub_root(install.path());
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    use_into(&hub, &first.path().display().to_string(), "demo@0.1.0");
    use_into(&hub, &second.path().display().to_string(), "demo@0.1.0");
    release_as(install.path(), &hub, "1.0.0");
    // 登记簿写盘路径被目录占住：实跑第一条即失败，后面的记录还没开始
    std::fs::create_dir_all(install.path().join("projects.toml.tmp")).unwrap();

    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade", "--all"]);

    assert_eq!(code, 1, "失败即停要退出码 1: {out}");
    let d = data_of(&out);
    assert!(
        d.get("upgraded").is_none(),
        "第一条就失败，没有已完成的记录"
    );
    assert_eq!(d["stopped"]["code"], serde_json::json!("io"));
    assert_eq!(d["stopped"]["key"], serde_json::json!("demo"));
    let not_started = d["stopped"]["not_started"].as_array().unwrap();
    assert_eq!(not_started.len(), 1, "另一条记录未开始: {not_started:?}");
    assert!(not_started[0].as_str().unwrap().contains("demo"));
}

#[test]
fn upgrade_all_reports_dead_project_and_archived_skips() {
    let (install, src) = env_with_versioned_skill("0.1.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let keeper = tempfile::tempdir().unwrap();
    let project_dir = project.path().display().to_string();
    let keeper_dir = keeper.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    use_into(&hub, &project_dir, "demo@0.1.0");
    use_into(&hub, &keeper_dir, "demo@0.1.0");
    release_as(install.path(), &hub, "1.0.0");
    // 登记簿留着两条记录，盘的现状却是项目目录没了一个、工作副本也没了：
    // 一条是死记录（归 gc），一条是归档态（先 restore）
    std::fs::remove_dir_all(project.path()).unwrap();
    std::fs::remove_dir_all(install.path().join("store/demo")).unwrap();

    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade", "--all"]);

    assert_eq!(code, 0, "跳过不算失败: {out}");
    let d = data_of(&out);
    assert!(d.get("upgraded").is_none(), "一条都没升: {out}");
    let skipped = d["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2, "两条各报一行: {skipped:?}");
    let dead = skipped
        .iter()
        .find(|s| s.get("key").is_none())
        .expect("死项目按整条记录报出，没有单个技能键");
    assert!(
        dead["reason"].as_str().unwrap().contains("gc"),
        "死记录文案要指向 gc: {}",
        dead["reason"]
    );
    let archived = skipped
        .iter()
        .find(|s| s["key"] == serde_json::json!("demo"))
        .expect("归档技能要报出");
    assert!(
        archived["reason"].as_str().unwrap().contains("归档"),
        "归档技能要说明原因: {}",
        archived["reason"]
    );
}

#[test]
fn upgrade_all_rejects_skill_and_all_together() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let hub = hub_root(install.path());
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );

    // 两个都给的用法错误在参数解析层挡下：稳定码是 usage，不是业务错误
    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade", "--all", "demo"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("usage"));

    // 两个都不给同样是用法错误
    let (out, code) = fsh(&["--json", "--hub", &hub, "upgrade"]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("usage"));
}
