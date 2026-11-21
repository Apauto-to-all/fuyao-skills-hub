//! 批量撤回：一条命令撤走一整套技能在多目标里的落点。
//! 没有可撤记录的键进 skipped，其余照常撤；退出码随「做没做成」走。

mod common;

use common::{Run, data_of, error_of, home_env, hub_root, init_hub_at, json, json_env, text_env};

/// 建一个已 init 的 hub，`k1/k2` 下有两个技能 alpha 与 beta。
/// 入库仍是单技能形状：本票只改撤回的入口
fn hub_with_pack() -> (tempfile::TempDir, String) {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    for name in ["alpha", "beta"] {
        let src = install.path().join("src").join(name);
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            src.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: 演示\n---\n\n正文\n"),
        )
        .unwrap();
        assert_eq!(
            json(
                &hub,
                &["add", &src.display().to_string(), "--prefix", "k1/k2"]
            )
            .code,
            0,
            "add {name} 夹具应成功"
        );
    }
    (install, hub)
}

/// 把整套里的某个技能铺给一个项目
fn use_in_project(hub: &str, name: &str, agents: &str, proj: &str) {
    assert_eq!(
        json(
            hub,
            &[
                "use", "--prefix", "k1/k2", "--skill", name, "-a", agents, "-p", proj
            ]
        )
        .code,
        0,
        "use k1/k2/{name} 夹具应成功"
    );
}

/// 一条命令把整套撤出全局目标：逐个技能各摘各的记录，applied 一次报全
#[test]
fn unuse_takes_the_whole_set_from_the_global_target() {
    let (install, hub) = hub_with_pack();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    for name in ["alpha", "beta"] {
        assert_eq!(
            json_env(
                &home_env(&home_s),
                &hub,
                &[
                    "use", "--prefix", "k1/k2", "--skill", name, "--global", "--agent", "dsh"
                ]
            )
            .code,
            0
        );
        assert!(home.path().join(".dsh/skills").join(name).is_dir());
    }

    let Run {
        stdout: out, code, ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &[
            "unuse", "--prefix", "k1/k2", "--all", "--agent", "dsh", "--global",
        ],
    );
    assert_eq!(code, 0, "整套撤回: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/k2/alpha", "k1/k2/beta"]),
        "整批做掉的键一次报全: {d}"
    );
    assert_eq!(d["skipped"], serde_json::json!([]), "{d}");
    for name in ["alpha", "beta"] {
        assert!(!home.path().join(".dsh/skills").join(name).exists());
    }
    let registry =
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap_or_default();
    assert!(
        !registry.contains("[[globals.skills]]"),
        "点满整条记录即整条撤回: {registry}"
    );
}

/// 一条命令把整套撤出一个项目：--skill 点名两个名字，键由前缀拼出
#[test]
fn unuse_takes_the_named_skills_from_a_project() {
    let (install, hub) = hub_with_pack();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    for name in ["alpha", "beta"] {
        use_in_project(&hub, name, "dsh", &proj);
    }

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "unuse", "--prefix", "k1/k2", "--skill", "alpha", "--skill", "beta", "-a", "dsh", "-p",
            &proj,
        ],
    );
    assert_eq!(code, 0, "整套撤回: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/k2/alpha", "k1/k2/beta"]),
        "{d}"
    );
    for name in ["alpha", "beta"] {
        assert!(!project.path().join(".dsh/skills").join(name).exists());
    }
    let registry =
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap_or_default();
    assert!(!registry.contains("[[projects."), "{registry}");
}

/// 个别技能没有可撤记录：进 skipped，其余照常撤，退出码仍为 0
#[test]
fn unuse_skips_the_unused_skills_and_keeps_going() {
    let (install, hub) = hub_with_pack();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    use_in_project(&hub, "alpha", "universal", &proj);

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "unuse",
            "--prefix",
            "k1/k2",
            "--all",
            "-a",
            "universal",
            "-p",
            &proj,
        ],
    );
    assert_eq!(code, 0, "有做成的就不算失败: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!(["k1/k2/alpha"]), "{d}");
    assert_eq!(
        d["skipped"][0]["key"],
        serde_json::json!("k1/k2/beta"),
        "{d}"
    );
    assert_eq!(
        d["skipped"][0]["code"],
        serde_json::json!("not_registered"),
        "{d}"
    );
    assert!(!project.path().join(".agents/skills/alpha").exists());
    let registry =
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap_or_default();
    assert!(!registry.contains("alpha"), "做成的记录要撤净: {registry}");
    assert!(
        install.path().join("store/k1/k2/beta").is_dir(),
        "跳过的技能工作副本不动"
    );
}

/// 整套都没有可撤记录：applied 空而 skipped 非空 → 退出码 1
#[test]
fn unuse_exits_with_one_when_the_whole_set_is_skipped() {
    let (_install, hub) = hub_with_pack();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "unuse",
            "--prefix",
            "k1/k2",
            "--all",
            "-a",
            "universal",
            "-p",
            &proj,
        ],
    );
    assert_eq!(code, 1, "一条都没做成: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!([]), "{d}");
    assert_eq!(d["skipped"].as_array().unwrap().len(), 2, "{d}");
}

/// 共用落点在批量下同样留到最后一个引用它的 agent 撤走时才删：
/// alpha 的目录还有 kimi-code-cli 在用，beta 的目录只有 universal 一人
#[test]
fn unuse_keeps_a_shared_destination_until_the_last_agent_leaves() {
    let (install, hub) = hub_with_pack();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    use_in_project(&hub, "alpha", "universal,kimi-code-cli", &proj);
    use_in_project(&hub, "beta", "universal", &proj);
    let shared = project.path().join(".agents/skills/alpha");
    let own = project.path().join(".agents/skills/beta");

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "unuse",
            "--prefix",
            "k1/k2",
            "--all",
            "-a",
            "universal",
            "-p",
            &proj,
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(shared.is_dir(), "kimi-code-cli 还在用，共用落点不得删");
    assert!(!own.exists(), "只有一个引用者的落点该删");
    let registry = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert!(registry.contains("kimi-code-cli"), "{registry}");
    assert!(!registry.contains("universal"), "{registry}");

    // 最后一个引用者也撤走：目录与记录一起清掉
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "unuse",
            "--prefix",
            "k1/k2",
            "--skill",
            "alpha",
            "-a",
            "kimi-code-cli",
            "-p",
            &proj,
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(!shared.exists());
}

/// 预演逐条列出将删的落点与被跳过的键，不落盘
#[test]
fn unuse_dry_run_lists_actions_and_skips() {
    let (_install, hub) = hub_with_pack();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    use_in_project(&hub, "alpha", "universal", &proj);

    let Run {
        stdout: out, code, ..
    } = text_env(
        &[],
        &hub,
        &[
            "unuse",
            "--prefix",
            "k1/k2",
            "--all",
            "-a",
            "universal",
            "-p",
            &proj,
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "预演: {out}");
    let lines: Vec<&str> = out.lines().collect();
    // 链接级落点由本机降级链定档，符号链接与目录联接都可能
    assert!(
        lines[0].starts_with("- remove(symlink) ") || lines[0].starts_with("- remove(junction) "),
        "预演要标出落点形态: {out}"
    );
    assert!(lines[0].contains("alpha"), "{out}");
    assert!(
        lines[1].starts_with("- 跳过 k1/k2/beta（not_registered）"),
        "{out}"
    );
    assert!(
        project.path().join(".agents/skills/alpha").is_dir(),
        "预演不得删"
    );
}

/// 零兼容：旧的位置参数整键不再收；`--all` 没有前缀也没有入口
#[test]
fn the_old_positional_key_and_prefixless_all_are_usage_errors() {
    let (_install, hub) = hub_with_pack();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    for args in [
        vec!["unuse", "k1/k2/alpha", "-a", "universal", "-p", &proj],
        vec!["unuse", "--all", "-a", "universal", "-p", &proj],
    ] {
        let Run {
            stdout: out, code, ..
        } = json(&hub, &args);
        assert_eq!(code, 1, "{args:?} 应报用法错误: {out}");
        assert_eq!(
            error_of(&out)["code"],
            serde_json::json!("usage"),
            "{args:?}: {out}"
        );
    }
}
