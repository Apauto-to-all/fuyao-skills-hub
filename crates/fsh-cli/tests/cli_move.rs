//! 键迁移（move/mv）的成功路径：真实二进制上的目录挪动、登记改写与落点重链。

mod common;

use common::{Run, data_of, has_step, hub_with_nested_tdd, json, link_targets};

/// 成功迁移：目录在新位置、登记引用全部指向新键、junction 落点重指、
/// copy 落点字节不动、提交消息只含相对键
#[test]
fn move_relocates_registry_and_relinks_destinations() {
    let (install, hub, _src) = hub_with_nested_tdd();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    // 两个 agent 铺同一技能：universal（junction）落项目，dsh 由登记簿改写模拟 copy 位
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "--prefix",
                "a/b",
                "--skill",
                "tdd",
                "-a",
                "universal",
                "-p",
                &proj
            ]
        )
        .code,
        0,
        "use 夹具应成功"
    );
    // 在登记簿里补一条 copy 记录（copy 落点 + 内容改过）：move 后字节不动
    let copy_dest = project.path().join(".dsh/skills/tdd");
    std::fs::create_dir_all(&copy_dest).unwrap();
    std::fs::write(
        copy_dest.join("SKILL.md"),
        "---\nname: tdd\ndescription: 手改快照\n---\n",
    )
    .unwrap();
    let reg_path = install.path().join("projects.toml");
    // 追加一条 copy 记录到同一项目（直接拼 TOML，落点手造）。
    // 表键要用与 use 相同的规范化绝对路径（Windows 下带 \\?\ 前缀），否则会拼出新表
    let canon = std::fs::canonicalize(project.path())
        .unwrap()
        .display()
        .to_string();
    let copy_record = format!(
        "\n[[projects.\"{}\".skills]]\nkey = \"a/b/tdd\"\nagents = [\"dsh\"]\nlink = \"copy\"\n",
        canon.replace('\\', "\\\\")
    );
    let mut text = std::fs::read_to_string(&reg_path).unwrap();
    text.push_str(&copy_record);
    std::fs::write(&reg_path, text).unwrap();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["move", "--prefix", "a/b", "--skill", "tdd", "--to", "c/d"],
    );
    assert_eq!(code, 0, "move: {out}");
    let d = data_of(&out);
    assert_eq!(d["from"], serde_json::json!("a/b/tdd"));
    assert_eq!(d["to"], serde_json::json!("c/d/tdd"));
    assert_eq!(
        d["registry_edits"],
        serde_json::json!(2),
        "两条记录都要改写: {out}"
    );
    let relinked = d["relinked"].as_array().unwrap();
    assert_eq!(
        relinked.len(),
        1,
        "只有 junction 落点重链，copy 不入清单: {out}"
    );
    assert!(
        relinked[0]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with(".agents/skills/tdd"),
        "重链落点是 junction 落点路径: {out}"
    );

    // 目录挪到新位置，旧叶子不在（挪动只搬叶子，空的前缀父层留下无害）
    assert!(install.path().join("store/c/d/tdd/SKILL.md").is_file());
    assert!(!install.path().join("store/a/b/tdd").exists());

    // junction 落点路径不变、靶子指向新 store 路径
    let junction_dest = project.path().join(".agents/skills/tdd");
    assert!(junction_dest.is_dir(), "落点路径不变");
    assert!(
        link_targets(&junction_dest, &install.path().join("store/c/d/tdd")),
        "junction 靶子应指新 store 路径"
    );
    // copy 落点字节不动
    assert_eq!(
        std::fs::read_to_string(copy_dest.join("SKILL.md")).unwrap(),
        "---\nname: tdd\ndescription: 手改快照\n---\n",
        "copy 落点一个字节不动"
    );

    // 登记簿里再无旧键，两条记录的 agents/link 原样
    let reg = std::fs::read_to_string(&reg_path).unwrap();
    assert!(!reg.contains("a/b/tdd"), "登记簿不得再有旧键: {reg}");
    assert!(reg.contains("\"c/d/tdd\""));
    assert!(reg.contains("\"universal\""));
    assert!(reg.contains("\"dsh\""));
    assert!(reg.contains("link = \"copy\""));

    // 提交消息只含相对键：从 reflog（.git/logs/HEAD，机器提交逐条落账）核对
    let reflog = std::fs::read_to_string(install.path().join(".git/logs/HEAD")).unwrap();
    assert!(
        reflog.contains("fsh <fsh@local>") && reflog.contains("move(store): a/b/tdd -> c/d/tdd"),
        "应有一条 fsh 的 move 提交: {reflog}"
    );
}

/// 新前缀已存在时并入：前缀是命名空间，不是实体，已有内容不拦同名的另一套
#[test]
fn move_merges_into_an_existing_prefix() {
    let (install, hub, _src) = hub_with_nested_tdd();
    // 目标前缀下先有别的技能：前缀已在用，并入不该被拦
    let other = tempfile::tempdir().unwrap();
    let src = other.path().join("db");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: db\ndescription: 演示\n---\n",
    )
    .unwrap();
    assert_eq!(
        json(
            &hub,
            &["add", &src.display().to_string(), "--prefix", "c/d"]
        )
        .code,
        0
    );

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["move", "--prefix", "a/b", "--skill", "tdd", "--to", "c/d"],
    );
    assert_eq!(code, 0, "并入已存在的前缀: {out}");
    let d = data_of(&out);
    assert_eq!(d["from"], serde_json::json!("a/b/tdd"));
    assert_eq!(d["to"], serde_json::json!("c/d/tdd"));
    assert!(install.path().join("store/c/d/tdd/SKILL.md").is_file());
    assert!(
        install.path().join("store/c/d/db/SKILL.md").is_file(),
        "同前缀下已有的技能不受影响"
    );
    assert!(!install.path().join("store/a/b/tdd").exists());
}

/// 全局引用一并改写、全局落点一并重链：move 不挑作用域
#[test]
fn move_rewrites_global_references_too() {
    let (install, hub, _src) = hub_with_nested_tdd();
    let home = tempfile::tempdir().unwrap();
    let home_s = home.path().display().to_string();
    let env = common::home_env(&home_s);
    assert_eq!(
        common::json_env(
            &env,
            &hub,
            &[
                "use",
                "--prefix",
                "a/b",
                "--skill",
                "tdd",
                "--global",
                "-a",
                "universal",
            ]
        )
        .code,
        0,
        "use --global 夹具应成功"
    );
    let reg_path = install.path().join("projects.toml");
    assert!(
        std::fs::read_to_string(&reg_path)
            .unwrap()
            .contains("[[globals.skills]]"),
        "夹具应落下全局记录"
    );

    let Run {
        stdout: out, code, ..
    } = common::json_env(
        &env,
        &hub,
        &["move", "--prefix", "a/b", "--skill", "tdd", "--to", "c/d"],
    );
    assert_eq!(code, 0, "move: {out}");
    let d = data_of(&out);
    assert_eq!(d["registry_edits"], serde_json::json!(1), "{out}");
    assert_eq!(
        d["relinked"].as_array().unwrap().len(),
        1,
        "全局落点也要重链: {out}"
    );

    let reg = std::fs::read_to_string(&reg_path).unwrap();
    assert!(!reg.contains("a/b/tdd"), "全局段不得留旧键: {reg}");
    assert!(reg.contains("\"c/d/tdd\""), "{reg}");
    // 落点路径不变、靶子指向新 store 路径
    let dest = home.path().join(".agents/skills/tdd");
    assert!(
        link_targets(&dest, &install.path().join("store/c/d/tdd")),
        "全局落点应指新 store 路径"
    );
}

/// dry-run：动作清单齐全（挪目录 + 改登记 + 重链落点 + 提交），零落盘
#[test]
fn move_dry_run_lists_actions_without_touching_disk() {
    let (install, hub, _src) = hub_with_nested_tdd();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "--prefix",
                "a/b",
                "--skill",
                "tdd",
                "-a",
                "universal",
                "-p",
                &proj
            ]
        )
        .code,
        0
    );
    let store_before =
        std::fs::read_to_string(install.path().join("store/a/b/tdd/SKILL.md")).unwrap();
    let reg_before = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();
    let junction_dest = project.path().join(".agents/skills/tdd");

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "mv", "--prefix", "a/b", "--skill", "tdd", "--to", "c/b", "-n",
        ],
    );
    assert_eq!(code, 0, "dry-run: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    assert_eq!(d["from"], serde_json::json!("a/b/tdd"));
    assert_eq!(
        d["to"],
        serde_json::json!("c/b/tdd"),
        "预演也报得出新键: {out}"
    );
    let actions = &d["actions"];
    assert!(
        has_step(
            actions,
            serde_json::json!({"action": "move", "from": "store/a/b/tdd", "to": "store/c/b/tdd"})
        ),
        "清单要列挪目录: {actions}"
    );
    assert!(
        has_step(actions, serde_json::json!({"action": "register"})),
        "清单要列改登记: {actions}"
    );
    assert!(
        has_step(
            actions,
            serde_json::json!({"action": "link", "agents": ["universal"]})
        ),
        "清单要列重链落点: {actions}"
    );
    assert!(
        has_step(actions, serde_json::json!({"action": "commit"})),
        "清单要列提交: {actions}"
    );

    // 零落盘
    assert_eq!(
        std::fs::read_to_string(install.path().join("store/a/b/tdd/SKILL.md")).unwrap(),
        store_before
    );
    assert!(!install.path().join("store/c/b/tdd").exists());
    assert_eq!(
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap(),
        reg_before
    );
    // junction 落点原样指着旧 store 路径
    assert!(link_targets(
        &junction_dest,
        &install.path().join("store/a/b/tdd")
    ));
}
