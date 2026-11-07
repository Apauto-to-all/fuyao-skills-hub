//! 项目使用与撤回：真实二进制上的落点、引用与失败码。

mod common;

use common::{data_of, env_with_versioned_skill, error_of, fsh, hub_root};
#[test]
fn use_and_unuse_round_trip_through_real_binary() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let src_s = src.display().to_string();

    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(fsh(&["--json", "--hub", &hub, "add", &src_s]).1, 0);

    // 预演：只列动作，不落盘
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo@1.0.0",
        "--agent",
        "universal,dsh",
        "--project",
        &proj,
        "--dry-run",
    ]);
    assert_eq!(code, 0, "use --dry-run: {out}");
    assert_eq!(data_of(&out)["dry_run"], serde_json::json!(true));
    assert!(!install.path().join("export/demo/1.0.0").exists());

    // 实跑：固定版本导出 + 两个落点
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo@1.0.0",
        "--agent",
        "universal,dsh",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 0, "use: {out}");
    let d = data_of(&out);
    assert_eq!(d["version"], serde_json::json!("1.0.0"));
    assert_eq!(d["targets"][0]["destinations"].as_array().unwrap().len(), 2);
    assert_eq!(d["targets"][0]["scope"], serde_json::json!("project"));
    for name in [".agents/skills/demo", ".dsh/skills/demo"] {
        assert!(
            project.path().join(name).join("SKILL.md").is_file(),
            "{name}"
        );
    }

    // 重复 use：拒绝且给出稳定码
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--agent",
        "universal",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("already_registered")
    );

    // 非法技能说明：稳定码 invalid_skill_spec
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo@1.0",
        "--agent",
        "universal",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("invalid_skill_spec")
    );

    // 未知名 agent
    let other = tempfile::tempdir().unwrap();
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--agent",
        "nope",
        "--project",
        &other.path().display().to_string(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("unknown_agent"));

    // 撤回预演后实跑：落点与记录都清掉
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "unuse",
        "demo",
        "--project",
        &proj,
        "--dry-run",
    ]);
    assert_eq!(code, 0, "unuse --dry-run: {out}");
    assert!(
        project.path().join(".dsh/skills/demo").is_dir(),
        "预演不得删"
    );
    let (out, code) = fsh(&["--json", "--hub", &hub, "unuse", "demo", "--project", &proj]);
    assert_eq!(code, 0, "unuse: {out}");
    assert_eq!(
        data_of(&out)["targets"][0]["removed"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!project.path().join(".agents/skills/demo").exists());

    // 再撤回一次：没有记录，稳定码 not_registered
    let (out, code) = fsh(&["--json", "--hub", &hub, "unuse", "demo", "--project", &proj]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("not_registered"));
}

#[test]
fn use_failure_codes_are_stable_over_real_binary() {
    let (install, src) = env_with_versioned_skill("1.0.0");
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );

    // 未初始化 hub
    let bare = install.path().join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &bare.display().to_string(),
        "use",
        "demo",
        "--agent",
        "universal",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("hub_not_initialized")
    );

    // 项目目录不存在
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--agent",
        "universal",
        "--project",
        &project.path().join("nope").display().to_string(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("invalid_project"));

    // 落点被占用
    let occupied = project.path().join(".agents/skills/demo");
    std::fs::create_dir_all(&occupied).unwrap();
    std::fs::write(occupied.join("user.txt"), "keep").unwrap();
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo",
        "--agent",
        "universal",
        "--project",
        &proj,
    ]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("destination_occupied")
    );
    assert!(occupied.join("user.txt").is_file(), "占用目录不得被覆盖");
    std::fs::remove_dir_all(&occupied).unwrap();

    // 固定版本导出后手改成脏目录：再次使用必须 dirty_export
    assert_eq!(
        fsh(&[
            "--json",
            "--hub",
            &hub,
            "use",
            "demo@1.0.0",
            "--agent",
            "universal",
            "--project",
            &proj,
        ])
        .1,
        0
    );
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "unuse", "demo", "--project", &proj]).1,
        0
    );
    let export = install.path().join("export/demo/1.0.0");
    std::fs::create_dir_all(&export).unwrap();
    std::fs::write(export.join("hand-edit.txt"), "dirty").unwrap();
    let other = tempfile::tempdir().unwrap();
    let (out, code) = fsh(&[
        "--json",
        "--hub",
        &hub,
        "use",
        "demo@1.0.0",
        "--agent",
        "universal",
        "--project",
        &other.path().display().to_string(),
    ]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("dirty_export"));
    assert!(export.join("hand-edit.txt").is_file(), "脏目录绝不覆盖");
}
