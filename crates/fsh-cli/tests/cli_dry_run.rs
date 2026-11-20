//! 预演：动作清单与真跑效果一致，且零落盘。

mod common;

use common::{Run, data_of, env_with_source, has_step, hub_root, init_hub_at, json};

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
    } = json(&hub, &["archive", "--skill", "demo", "--dry-run"]);
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
    for args in [vec!["archive", "--skill", "demo"], vec!["remove", "demo"]] {
        let mut short = args.clone();
        short.push("-n");
        let mut long = args.clone();
        long.push("--dry-run");
        let Run {
            stdout: short_out,
            code,
            ..
        } = json(&hub, &short);
        assert_eq!(code, 0, "{args:?} -n: {short_out}");
        let Run {
            stdout: long_out, ..
        } = json(&hub, &long);
        assert_eq!(
            data_of(&short_out),
            data_of(&long_out),
            "{args:?} -n 与 --dry-run 的 data 应相同"
        );
    }

    // 归档后 restore 的预演
    assert_eq!(json(&hub, &["archive", "--skill", "demo"]).code, 0);
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

/// 一条不变量覆盖全部写命令：预演列出的提交步，必须与实跑真正产生的提交
/// 逐字一致（消息与顺序）。预演清单不是另一套描述，提交步就写在计划里
#[test]
fn dry_run_commit_steps_match_the_commits_each_real_run_makes() {
    let (install, src) = env_with_source();
    let src_s = src.display().to_string();
    let hub = hub_root(install.path());
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    // init 首建：装配与内置对齐同在一份计划里落盘，只提一条提交——预演要说全
    assert_dry_matches_history(&hub, &["init", "--dry-run"], &["init"]);
    assert_eq!(
        commit_subjects(&hub),
        vec!["init(hub): hub"],
        "一次 init 只提一条提交"
    );

    // 库管理面：每条命令一条提交
    assert_dry_matches_history(&hub, &["add", &src_s, "--dry-run"], &["add", &src_s]);
    assert_dry_matches_history(
        &hub,
        &["archive", "--skill", "demo", "--dry-run"],
        &["archive", "--skill", "demo"],
    );
    assert_dry_matches_history(
        &hub,
        &["restore", "demo", "--dry-run"],
        &["restore", "demo"],
    );
    assert_dry_matches_history(
        &hub,
        &["move", "--skill", "demo", "--to", "org", "--dry-run"],
        &["move", "--skill", "demo", "--to", "org"],
    );

    // 项目面：落点建在项目目录里，登记簿是 ignored 文件，hub 仓库没有变更，
    // 预演与实跑因此都没有提交可列
    assert_dry_matches_history(
        &hub,
        &[
            "use",
            "--prefix",
            "org",
            "--skill",
            "demo",
            "-a",
            "universal",
            "-p",
            &proj,
            "--dry-run",
        ],
        &[
            "use",
            "--prefix",
            "org",
            "--skill",
            "demo",
            "-a",
            "universal",
            "-p",
            &proj,
        ],
    );
    assert_dry_matches_history(
        &hub,
        &[
            "unuse",
            "--prefix",
            "org",
            "--skill",
            "demo",
            "-a",
            "universal",
            "-p",
            &proj,
            "--dry-run",
        ],
        &[
            "unuse",
            "--prefix",
            "org",
            "--skill",
            "demo",
            "-a",
            "universal",
            "-p",
            &proj,
        ],
    );

    // 收尾：删掉之后没有可提交的变更
    assert_dry_matches_history(
        &hub,
        &["remove", "org/demo", "--dry-run"],
        &["remove", "org/demo"],
    );
}

/// 脏 hub：手改过工作副本时，实跑会在动盘前先提交一次手改快照。
/// 预演必须把这条也列出来——store 是编辑面，手改是常规路径而非边角
#[test]
fn dry_run_lists_the_manual_snapshot_commit_on_a_dirty_hub() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let src_s = src.display().to_string();
    assert_eq!(json(&hub, &["add", &src_s]).code, 0);

    // 手改工作副本：hub 仓库因此变脏
    std::fs::write(
        install.path().join("store/demo/SKILL.md"),
        "---\nname: demo\ndescription: 手改过的描述\n---\n\n正文\n",
    )
    .unwrap();

    assert_dry_matches_history(
        &hub,
        &["archive", "--skill", "demo", "--dry-run"],
        &["archive", "--skill", "demo"],
    );
    // 实跑确实产生了那条快照提交，且排在本命令自身提交之前
    let history = commit_subjects(&hub);
    let tail = &history[history.len() - 2..];
    assert_eq!(
        tail,
        ["sync(store): manual", "archive(store): 根 1 个技能"],
        "手改快照先提交，命令自身的提交在后"
    );
}

/// 一条命令的不变量：预演的提交步 == 实跑新增的提交。消息与顺序都要对上
fn assert_dry_matches_history(hub: &str, dry: &[&str], real: &[&str]) {
    let before = commit_subjects(hub);
    let Run { stdout, code, .. } = json(hub, dry);
    assert_eq!(code, 0, "{dry:?} 预演应成功: {stdout}");
    let planned = planned_commits(&data_of(&stdout));
    let Run { stdout, code, .. } = json(hub, real);
    assert_eq!(code, 0, "{real:?} 实跑应成功: {stdout}");
    let after = commit_subjects(hub);
    let produced = &after[before.len().min(after.len())..];
    assert_eq!(
        planned, produced,
        "{real:?}: 预演列出的提交步要与实跑产生的提交逐字一致"
    );
}

/// hub 仓库的机器提交消息，从旧到新。尚未建出仓库时按「没有提交」处理
fn commit_subjects(hub: &str) -> Vec<String> {
    let Ok(repo) = fsh_core::git::open_hub(std::path::Path::new(hub)) else {
        return Vec::new();
    };
    let mut walk = repo.revwalk().unwrap();
    walk.push_head().unwrap();
    let mut subjects: Vec<String> = walk
        .map(|oid| {
            repo.find_commit(oid.unwrap())
                .unwrap()
                .message()
                .unwrap_or_default()
                .trim_end()
                .to_string()
        })
        .collect();
    subjects.reverse();
    subjects
}

/// 预演载荷里的提交步：顶层 `actions`（库管理面）与逐目标 `actions`（项目面）
/// 两处形状都收，取其中 `action == commit` 的步骤对象
fn planned_commits(data: &serde_json::Value) -> Vec<String> {
    let mut steps: Vec<&serde_json::Value> = Vec::new();
    if let Some(list) = data["actions"].as_array() {
        steps.extend(list);
    }
    if let Some(targets) = data["targets"].as_array() {
        for target in targets {
            if let Some(list) = target["actions"].as_array() {
                steps.extend(list);
            }
        }
    }
    steps
        .iter()
        .filter(|step| step["action"] == "commit")
        .map(|step| step["target"].as_str().unwrap_or_default().to_string())
        .collect()
}
