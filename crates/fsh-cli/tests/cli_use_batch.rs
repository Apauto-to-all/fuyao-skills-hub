//! 批量铺装：一条命令把一整套技能铺给全局或项目，跳过项逐条报告。
//! 断言只经真实二进制的信封与盘面：`applied` / `skipped` 清单、落点、登记簿。

mod common;

use std::fs;
use std::path::PathBuf;

use common::{
    Run, data_of, error_of, fsh, home_env, hub_root, init_hub_at, json, json_env, text_env,
};

/// 建一套技能的源目录：一个名字一个目录，各含带 frontmatter 的 SKILL.md
fn pack(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let root = tempfile::tempdir().unwrap();
    let dirs = names
        .iter()
        .map(|name| {
            let dir = root.path().join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: 演示\n---\n\n正文\n"),
            )
            .unwrap();
            dir
        })
        .collect();
    (root, dirs)
}

/// 把一套技能按前缀逐个入库：add 一次修一个技能
fn add_pack(hub: &str, prefix: &str, dirs: &[PathBuf]) {
    for dir in dirs {
        let Run {
            stdout: out, code, ..
        } = json(
            hub,
            &["add", &dir.display().to_string(), "--prefix", prefix],
        );
        assert_eq!(code, 0, "add {prefix}: {out}");
    }
}

/// 一套技能一条命令铺到全局：每个技能各建落点、各写一条登记记录
#[test]
fn whole_set_spreads_to_the_global_dirs_and_registers_each() {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let (_src, dirs) = pack(&["tdd", "review", "plan"]);
    add_pack(&hub, "k1/k2", &dirs);

    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let Run {
        stdout: out, code, ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &[
            "use",
            "--prefix",
            "k1/k2",
            "--all",
            "--agent",
            "universal,dsh",
            "--global",
        ],
    );
    assert_eq!(code, 0, "use --all --global: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/k2/plan", "k1/k2/review", "k1/k2/tdd"]),
        "整套按选择顺序铺完: {d}"
    );
    assert!(d["skipped"].as_array().unwrap().is_empty(), "{d}");
    for name in ["plan", "review", "tdd"] {
        for dir in [".agents/skills", ".dsh/skills"] {
            assert!(
                home.path().join(dir).join(name).join("SKILL.md").is_file(),
                "{dir}/{name} 应可读"
            );
        }
    }
    let registry = fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert_eq!(
        registry.matches("[[globals.skills]]").count(),
        3,
        "每个技能各一条记录: {registry}"
    );
}

/// 一套技能一条命令铺给两个项目：前段边界按段认，后代技能也在套里
#[test]
fn whole_set_spreads_to_two_projects_and_respects_the_prefix_boundary() {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let (_src, dirs) = pack(&["tdd", "sql"]);
    add_pack(&hub, "k1/k2", &dirs[..1]);
    // 更深的层同样在 k1/k2 这套里
    add_pack(&hub, "k1/k2/db", &dirs[1..]);
    // 相邻前缀 k1/k2b 不在这套里
    let (_other, other_dirs) = pack(&["other"]);
    add_pack(&hub, "k1/k2b", &other_dirs);

    let p1 = tempfile::tempdir().unwrap();
    let p2 = tempfile::tempdir().unwrap();
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "use",
            "--prefix",
            "k1/k2",
            "--all",
            "--agent",
            "universal",
            "--project",
            &p1.path().display().to_string(),
            "--project",
            &p2.path().display().to_string(),
        ],
    );
    assert_eq!(code, 0, "use --all 两项目: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/k2/db/sql", "k1/k2/tdd"]),
        "按段边界取整套，相邻前缀不在套里: {d}"
    );
    for project in [&p1, &p2] {
        for name in ["sql", "tdd"] {
            assert!(
                project
                    .path()
                    .join(".agents/skills")
                    .join(name)
                    .join("SKILL.md")
                    .is_file(),
                "{} 应有 {name} 落点",
                project.path().display()
            );
        }
        assert!(
            !project.path().join(".agents/skills/other").exists(),
            "相邻前缀的技能不得被带上"
        );
    }
    let registry = fs::read_to_string(install.path().join("projects.toml")).unwrap();
    assert_eq!(
        registry.matches("last_used = ").count(),
        2,
        "两个项目各一个项目段: {registry}"
    );
    assert_eq!(
        registry.matches("key = ").count(),
        4,
        "两个项目各两条记录: {registry}"
    );
}

/// 个别技能已登记或落点被占：只跳过它们并报告，其余照常铺，退出码 0
#[test]
fn laid_out_skill_is_skipped_and_the_rest_still_spreads() {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let (_src, dirs) = pack(&["tdd", "review", "plan"]);
    add_pack(&hub, "k1/k2", &dirs);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    // tdd 先单独铺一次：它在登记簿里已有记录
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "use",
            "--prefix",
            "k1/k2",
            "--skill",
            "tdd",
            "--agent",
            "universal",
            "--project",
            &proj,
        ],
    );
    assert_eq!(code, 0, "先铺一个: {out}");
    // plan 的落点被用户文件占着
    let occupied = project.path().join(".agents/skills/plan");
    fs::create_dir_all(&occupied).unwrap();
    fs::write(occupied.join("user.txt"), "keep").unwrap();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "use",
            "--prefix",
            "k1/k2",
            "--all",
            "--agent",
            "universal",
            "--project",
            &proj,
        ],
    );
    assert_eq!(code, 0, "有做成的就不算失败: {out}");
    let d = data_of(&out);
    assert_eq!(d["applied"], serde_json::json!(["k1/k2/review"]), "{d}");
    let skipped = d["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2, "{d}");
    assert_eq!(skipped[0]["key"], serde_json::json!("k1/k2/plan"));
    assert_eq!(
        skipped[0]["code"],
        serde_json::json!("destination_occupied")
    );
    assert_eq!(skipped[1]["key"], serde_json::json!("k1/k2/tdd"));
    assert_eq!(skipped[1]["code"], serde_json::json!("already_registered"));
    assert!(project.path().join(".agents/skills/review").is_dir());
    assert_eq!(
        fs::read_to_string(occupied.join("user.txt")).unwrap(),
        "keep",
        "占用目录不得被覆盖"
    );
}

/// 整套都已铺过：一条都没做成、只有跳过项，退出码 1
#[test]
fn whole_set_already_laid_out_exits_1() {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let (_src, dirs) = pack(&["tdd", "review"]);
    add_pack(&hub, "k1/k2", &dirs);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let args = [
        "use",
        "--prefix",
        "k1/k2",
        "--all",
        "--agent",
        "universal",
        "--project",
        &proj,
    ];
    assert_eq!(json(&hub, &args).code, 0, "第一遍整套铺装应成功");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &args);
    assert_eq!(code, 1, "全部跳过时退出码 1: {out}");
    let d = data_of(&out);
    assert!(d["applied"].as_array().unwrap().is_empty(), "{d}");
    let skipped = d["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 2, "{d}");
    for item in skipped {
        assert_eq!(item["code"], serde_json::json!("already_registered"));
    }
}

/// 预演逐条列出动作与被跳过的项，两种模式都零落盘
#[test]
fn dry_run_lists_actions_and_skips_without_writing() {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let (_src, dirs) = pack(&["tdd", "review", "plan"]);
    add_pack(&hub, "k1/k2", &dirs);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "--prefix",
                "k1/k2",
                "--skill",
                "tdd",
                "--agent",
                "universal",
                "--project",
                &proj,
            ],
        )
        .code,
        0
    );
    let registry_before = fs::read_to_string(install.path().join("projects.toml")).unwrap();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "use",
            "--prefix",
            "k1/k2",
            "--all",
            "--agent",
            "universal",
            "--project",
            &proj,
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "预演: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/k2/plan", "k1/k2/review"])
    );
    assert_eq!(d["skipped"].as_array().unwrap().len(), 1, "{d}");
    let links = d["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| action["action"] == serde_json::json!("link"))
        .count();
    assert_eq!(links, 2, "两个待铺技能各一步建链接: {d}");
    // 零落盘：待铺的落点未建，登记簿一字不改
    assert!(!project.path().join(".agents/skills/plan").exists());
    assert!(!project.path().join(".agents/skills/review").exists());
    assert_eq!(
        fs::read_to_string(install.path().join("projects.toml")).unwrap(),
        registry_before
    );

    // 文本模式：动作逐条一行，跳过项也逐条一行
    let Run {
        stdout: out, code, ..
    } = text_env(
        &[],
        &hub,
        &[
            "use",
            "--prefix",
            "k1/k2",
            "--all",
            "--agent",
            "universal",
            "--project",
            &proj,
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "预演文本: {out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines.iter().any(|line| line.starts_with("- link ")),
        "预演逐条列出动作: {out}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("- 跳过 k1/k2/tdd（already_registered）")),
        "预演也要列出跳过项: {out}"
    );
}

/// `--all` 与 `--skill` 二选一必填，且 `--all` 必须有前缀：不存在整库批量铺装
#[test]
fn all_requires_a_prefix_and_one_of_the_two_pickers() {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);

    for args in [
        vec![
            "--json",
            "--hub",
            &hub,
            "use",
            "--all",
            "-a",
            "universal",
            "-g",
        ],
        vec!["--json", "--hub", &hub, "use", "-a", "universal", "-g"],
        vec![
            "--json",
            "--hub",
            &hub,
            "use",
            "--prefix",
            "k1",
            "--all",
            "--skill",
            "tdd",
            "-a",
            "universal",
            "-g",
        ],
    ] {
        let Run {
            stdout: out, code, ..
        } = fsh(&args);
        assert_eq!(code, 1, "{args:?} 应是用法错误: {out}");
        assert_eq!(error_of(&out)["code"], serde_json::json!("usage"), "{out}");
    }
}
