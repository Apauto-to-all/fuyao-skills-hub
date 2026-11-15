//! 短选项与子命令别名：只改输入，输出、错误码与退出码逐字不变。

mod common;

use common::{
    Run, data_of, env_with_source, error_of, fsh, home_env, hub_with_referenced_demo,
    initialized_hub, json, json_env,
};

/// ls 是 list 的可见别名：同一份 data，过滤词与 --archived 照旧可用
#[test]
fn ls_is_the_same_command_as_list() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);

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
            stdout: name_out,
            code,
            ..
        } = json(&hub, &name_args);
        assert_eq!(code, 0, "list {extra:?}: {name_out}");
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
        let hub = initialized_hub(&install, &src);
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
    let hub = initialized_hub(&install, &src);

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
