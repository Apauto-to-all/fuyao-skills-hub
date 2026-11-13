//! Seam 2 集成测试：起真实 fsh 子进程，断言 --json 信封、退出码 0/1 与磁盘真实效果。
//! 外部行为契约在此固化，不测任何内部实现。

mod common;

use std::path::Path;

use common::{
    Run, data_of, env_with_source, error_of, fsh, home_env, hub_root, init_hub_at, json, json_env,
};
#[test]
fn init_is_enveloped_and_idempotent() {
    let (install, _) = env_with_source();
    let hub = hub_root(install.path());
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["init"]);
    assert_eq!(code, 0);
    assert_eq!(data_of(&out)["initialized"], serde_json::json!(true));
    // 三个空目录一次到位：库、导出、agent 覆盖都留着待用
    for dir in ["store", "archive", "agents"] {
        assert!(install.path().join(dir).is_dir(), "{dir}/ 应由 init 建出");
    }

    // 重跑幂等：ok:true + already_initialized，一字节不动
    let mtime = |p: &Path| {
        std::fs::metadata(p)
            .unwrap()
            .modified()
            .unwrap()
            .elapsed()
            .unwrap()
            .as_secs()
    };
    let hub_toml = install.path().join("hub.toml");
    let before = mtime(&hub_toml);
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["init"]);
    assert_eq!(code, 0);
    assert_eq!(
        data_of(&out)["already_initialized"],
        serde_json::json!(true)
    );
    assert_eq!(mtime(&hub_toml), before, "幂等重跑不能改文件");

    // 未初始化的目录上直接跑业务命令 → 失败信封 + 退出码 1
    let empty = tempfile::tempdir().unwrap();
    let Run {
        stdout: out, code, ..
    } = json(&hub_root(empty.path()), &["list"]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("hub_not_initialized")
    );
}

#[test]
fn add_list_full_flow() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let Run { code, .. } = json(&hub, &["init"]);
    assert_eq!(code, 0);

    // add：一步到位入库
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src.display().to_string()]);
    assert_eq!(code, 0, "add: {out}");
    assert_eq!(data_of(&out)["key"], serde_json::json!("demo"));

    // list：一行一个技能，JSON 给 description 全文
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list"]);
    assert_eq!(code, 0, "list: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let demo = rows.iter().find(|r| r["key"] == "demo").unwrap();
    assert_eq!(demo["description"], serde_json::json!("演示"));
    assert_eq!(demo["uses"], serde_json::json!([]), "没用过就是空清单");
    // 位置过滤词收窄
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list", "demo"]);
    assert_eq!(code, 0);
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    assert!(rows.iter().all(|r| r["key"] == "demo"), "过滤后只剩 demo");
}

#[test]
fn archive_remove_lifecycle() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    json(&hub, &["add", &src.display().to_string()]);

    // 归档：工作副本整目录移入归档面
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "demo"]);
    assert_eq!(code, 0, "archive: {out}");
    assert!(!install.path().join("store/demo/SKILL.md").exists());
    assert!(
        install.path().join("archive/demo/SKILL.md").is_file(),
        "归档副本应落在 archive/demo"
    );
    // 归档技能只在归档旗标下列出
    let Run { stdout: out, .. } = json(&hub, &["list"]);
    let keys = |out: &str| {
        data_of(out)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["key"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert!(!keys(&out).contains(&"demo".to_string()), "缺省不列归档");
    let Run {
        stdout: archived_out,
        ..
    } = json(&hub, &["list", "--archived"]);
    assert!(keys(&archived_out).contains(&"demo".to_string()));

    // 恢复：移回 store
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["restore", "demo"]);
    assert_eq!(code, 0, "restore: {out}");
    assert!(install.path().join("store/demo/SKILL.md").is_file());
    assert!(!install.path().join("archive/demo").exists());

    // remove 成功后列表与归档面都没有它
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "demo"]);
    assert_eq!(code, 0, "remove: {out}");
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list", "--archived"]);
    assert_eq!(code, 0);
    assert!(!keys(&out).contains(&"demo".to_string()));
}

#[test]
fn remove_referenced_by_project_is_rejected() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    init_hub_at(&hub);
    json(&hub, &["add", &src.display().to_string()]);
    // 登记簿记一条项目引用（use 之后的状态）；路径用 TOML 字面量串，
    // Windows 反斜杠无需转义
    let registry = install.path().join("projects.toml");
    let project_path = project.path().display().to_string();
    std::fs::write(
        &registry,
        format!(
            "[projects.'{project_path}']\nlast_used = 1\n\n[[projects.'{project_path}'.skills]]\nkey = \"demo\"\nagents = [\"agents\"]\nlink = \"junction\"\n"
        ),
    )
    .unwrap();

    // 被引用的 skill 不许删：失败信封 + 退出码 1
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["remove", "demo"]);
    assert_eq!(code, 1, "remove 被引用: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("referenced"));

    // 归档同样拒绝，原因同一份登记簿
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["archive", "demo"]);
    assert_eq!(code, 1, "archive 被引用: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("referenced"));
}

#[test]
fn add_with_prefix_uses_nested_key() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    // --prefix 提供组织前缀：键 = 前缀/frontmatter name
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "add",
            &src.display().to_string(),
            "--prefix",
            "mattpocock/skills",
        ],
    );
    assert_eq!(code, 0, "add --prefix: {out}");
    assert_eq!(
        data_of(&out)["key"],
        serde_json::json!("mattpocock/skills/demo")
    );

    // 嵌套键出现在列表里
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list"]);
    assert_eq!(code, 0, "list: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let nested = rows
        .iter()
        .find(|r| r["key"] == "mattpocock/skills/demo")
        .expect("嵌套键应出现在列表");
    assert_eq!(nested["description"], serde_json::json!("演示"));
}

#[test]
fn unknown_command_gets_error_envelope_with_exit_1() {
    let Run {
        stdout: out, code, ..
    } = fsh(&["--json", "nope"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("usage"));

    // 文本模式下 --help 属正常输出，退出码 0
    let Run { code, .. } = fsh(&["--help"]);
    assert_eq!(code, 0);
}

/// init 的帮助要说清幂等语义：已初始化只报告，内容一致则不动
#[test]
fn init_help_says_content_matches_stays_untouched() {
    let Run {
        stdout: out, code, ..
    } = fsh(&["init", "--help"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("内容一致则不动"),
        "init 帮助应写明内容一致则不动: {out}"
    );
}

/// use 与 unuse 的 --agent 都必填：缺了进不了命令，两边的拼法同形
#[test]
fn use_and_unuse_require_the_agent_list() {
    for cmd in ["use", "unuse"] {
        let Run {
            stdout: out, code, ..
        } = fsh(&[cmd, "--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("--agent"), "{cmd} 帮助里应有 --agent: {out}");
        let Run {
            stdout: out, code, ..
        } = fsh(&["--json", cmd, "demo", "--global"]);
        assert_eq!(code, 1, "{cmd} 缺 --agent 应报用法错误: {out}");
        assert_eq!(
            error_of(&out)["code"],
            serde_json::json!("usage"),
            "{cmd}: {out}"
        );
    }
}

#[test]
fn help_lists_the_ten_commands() {
    let Run {
        stdout: out, code, ..
    } = fsh(&["--help"]);
    assert_eq!(code, 0);
    // 帮助的 Commands 区块逐行一条；末行 help 是 clap 自带的子命令
    let listed: Vec<&str> = out
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(listed.len(), 11, "命令面应恰好 10 条加 help: {listed:?}");
    for kept in [
        "init", "add", "list", "archive", "remove", "restore", "use", "unuse", "search", "doctor",
    ] {
        assert!(listed.contains(&kept), "帮助里应有 {kept}: {out}");
    }
}

/// 建一个登记簿里被项目引用的 hub：remove 与 rm 都按同一份登记簿拒绝
fn hub_with_referenced_demo() -> (tempfile::TempDir, tempfile::TempDir, String) {
    let (install, src) = env_with_source();
    let project = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);
    // 登记簿记一条项目引用；路径用 TOML 字面量串，Windows 反斜杠无需转义
    let registry = install.path().join("projects.toml");
    let project_path = project.path().display().to_string();
    std::fs::write(
        &registry,
        format!(
            "[projects.'{project_path}']\nlast_used = 1\n\n[[projects.'{project_path}'.skills]]\nkey = \"demo\"\nagents = [\"agents\"]\nlink = \"junction\"\n"
        ),
    )
    .unwrap();
    (install, project, hub)
}

/// ls 是 list 的可见别名：同一份 data，过滤词与 --archived 照旧可用
#[test]
fn ls_is_the_same_command_as_list() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);

    for extra in [&[][..], &["demo"][..], &["--archived"][..]] {
        let mut alias_args = vec!["ls"];
        alias_args.extend_from_slice(extra);
        let mut name_args = vec!["list"];
        name_args.extend_from_slice(extra);

        let Run {
            stdout: alias_out,
            code,
            ..
        } = json(&hub, &alias_args);
        assert_eq!(code, 0, "ls {extra:?}: {alias_out}");
        let Run {
            stdout: name_out, ..
        } = json(&hub, &name_args);
        assert_eq!(
            data_of(&alias_out),
            data_of(&name_out),
            "ls 与 list 的 data 应逐字相同: {extra:?}"
        );
    }
}

/// rm 是 remove 的可见别名：成功路径与被引用拒绝路径上行为逐字相同
#[test]
fn rm_is_the_same_command_as_remove() {
    // 成功路径：两条命令各在自己的 hub 上删同一个键，data 逐字相同
    let mut success = Vec::new();
    for cmd in ["rm", "remove"] {
        let (install, src) = env_with_source();
        let hub = hub_root(install.path());
        init_hub_at(&hub);
        assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);
        let Run {
            stdout: out, code, ..
        } = json(&hub, &[cmd, "demo"]);
        assert_eq!(code, 0, "{cmd}: {out}");
        assert!(!install.path().join("store/demo").exists(), "{cmd} 应真删");
        success.push(data_of(&out));
    }
    assert_eq!(
        success[0], success[1],
        "rm 与 remove 的成功 data 应逐字相同"
    );

    // 仍被引用：错误码与退出码相同，两处都点名引用关系
    let mut rejected = Vec::new();
    for cmd in ["rm", "remove"] {
        let (_install, _project, hub) = hub_with_referenced_demo();
        let Run {
            stdout: out, code, ..
        } = json(&hub, &[cmd, "demo"]);
        assert_eq!(code, 1, "{cmd} 被引用: {out}");
        let error = error_of(&out);
        assert!(
            error["message"].as_str().unwrap().contains("仍被引用"),
            "{cmd}: {out}"
        );
        rejected.push((error["code"].clone(), code));
    }
    assert_eq!(
        rejected[0], rejected[1],
        "rm 与 remove 的拒绝码与退出码应相同"
    );
    assert_eq!(rejected[0].0, serde_json::json!("referenced"));
}

/// 短名只改输入：use / unuse 的动作清单与长名逐字相同，长短混写也解析
#[test]
fn short_options_match_long_options_letter_for_letter() {
    let (install, src) = env_with_source();
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let proj = project.path().display().to_string();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src.display().to_string()]).code, 0);

    // use 的预演清单：全拼、全短名、长短混写三种写法同一份 data
    let full = json_env(
        &home_env(&home_s),
        &hub,
        &[
            "use",
            "demo",
            "--agent",
            "universal,dsh",
            "--global",
            "--dry-run",
        ],
    );
    assert_eq!(full.code, 0, "use 全拼: {}", full.stdout);
    let full = data_of(&full.stdout);
    for args in [
        vec!["use", "demo", "-a", "universal,dsh", "-g", "-n"],
        vec!["use", "demo", "--agent", "universal,dsh", "-g", "--dry-run"],
        vec!["use", "demo", "-a", "universal,dsh", "--global", "-n"],
    ] {
        let Run {
            stdout: out, code, ..
        } = json_env(&home_env(&home_s), &hub, &args);
        assert_eq!(code, 0, "{args:?}: {out}");
        assert_eq!(data_of(&out), full, "{args:?} 的 data 应与全拼相同");
    }

    // -p 与 --project：同一份 data
    let Run {
        stdout: full_project,
        code,
        ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &[
            "use",
            "demo",
            "--agent",
            "universal",
            "--project",
            &proj,
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "use --project: {full_project}");
    let Run {
        stdout: short_project,
        code,
        ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &["use", "demo", "-a", "universal", "-p", &proj, "-n"],
    );
    assert_eq!(code, 0, "use -p: {short_project}");
    assert_eq!(data_of(&short_project), data_of(&full_project));

    // 真铺一次，再比 unuse 的预演清单：-a / -p / -f / -n 一并覆盖
    assert_eq!(
        json(&hub, &["use", "demo", "-a", "universal", "-p", &proj]).code,
        0,
        "use 实跑应成功"
    );
    let Run {
        stdout: full_unuse,
        code,
        ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &[
            "unuse",
            "demo",
            "--agent",
            "universal",
            "--project",
            &proj,
            "--force",
            "--dry-run",
        ],
    );
    assert_eq!(code, 0, "unuse 全拼: {full_unuse}");
    let Run {
        stdout: short_unuse,
        code,
        ..
    } = json_env(
        &home_env(&home_s),
        &hub,
        &["unuse", "demo", "-a", "universal", "-p", &proj, "-f", "-n"],
    );
    assert_eq!(code, 0, "unuse 短名: {short_unuse}");
    assert_eq!(data_of(&short_unuse), data_of(&full_unuse));
}

/// -n 是 --dry-run 的短名：六条写命令上给出同一份动作清单
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

/// 帮助里看得见两个别名：命令名仍在首列，别名追加在描述末尾
#[test]
fn help_shows_ls_and_rm_as_visible_aliases() {
    let Run {
        stdout: out, code, ..
    } = fsh(&["--help"]);
    assert_eq!(code, 0);
    let commands: Vec<String> = out
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let line_of = |name: &str| {
        commands
            .iter()
            .find(|l| l.split_whitespace().next() == Some(name))
            .unwrap_or_else(|| panic!("Commands 区块应有 {name} 行: {out}"))
            .clone()
    };
    // 单个可见别名在帮助里渲染成单数形态，多个才用复数
    let list_line = line_of("list");
    assert!(
        list_line.contains("[alias: ls]"),
        "list 行应标出别名: {list_line}"
    );
    let remove_line = line_of("remove");
    assert!(
        remove_line.contains("[alias: rm]"),
        "remove 行应标出别名: {remove_line}"
    );
}

/// 未收的短名与任意前缀仍被拒：统一走 usage 失败信封与退出码 1
#[test]
fn unlisted_short_flags_and_prefixes_stay_usage_errors() {
    for args in [
        vec!["--json", "list", "-j"],
        vec!["--json", "in"],
        vec!["--json", "add", "x", "-p", "a/b"],
    ] {
        let Run {
            stdout: out, code, ..
        } = fsh(&args);
        assert_eq!(code, 1, "{args:?} 应按用法错误退出: {out}");
        assert_eq!(
            error_of(&out)["code"],
            serde_json::json!("usage"),
            "{args:?}: {out}"
        );
    }
}
