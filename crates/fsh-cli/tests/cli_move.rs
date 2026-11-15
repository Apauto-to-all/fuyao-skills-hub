//! 键迁移（move/mv）：真实二进制上的目录挪动、登记改写与落点重链。

mod common;

use common::{Run, data_of, error_of, has_step, hub_root, init_hub_at, json};
use std::path::Path;

/// 建一个已入库多级键技能的 hub：`fsh add <src> --prefix a/b` 得到 a/b/tdd。
/// 返回（hub 安装目录守卫，hub 根路径，技能源目录）
fn hub_with_nested_tdd() -> (tempfile::TempDir, String, std::path::PathBuf) {
    let install = tempfile::tempdir().unwrap();
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    let src_root = tempfile::tempdir().unwrap();
    let src = src_root.path().join("tdd");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: tdd\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    assert_eq!(
        json(
            &hub,
            &["add", &src.display().to_string(), "--prefix", "a/b"]
        )
        .code,
        0
    );
    (install, hub, src)
}

/// junction/symlink 落点的靶子是否指向给定目录（穿透后比对路径身份）
fn link_targets(dest: &Path, expected: &Path) -> bool {
    let a = std::fs::canonicalize(dest).unwrap();
    let b = std::fs::canonicalize(expected).unwrap();
    a == b
}

/// 成功迁移：目录在新位置、登记引用全部指向新键、junction 落点重指、
/// copy 落点字节不动、提交消息只含相对键
#[test]
fn move_relocates_registry_and_relinks_destinations() {
    let (install, hub, _src) = hub_with_nested_tdd();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    // 两个 agent 铺同一技能：universal（junction）落项目，dsh 由登记簿改写模拟 copy 位
    assert_eq!(
        json(&hub, &["use", "a/b/tdd", "-a", "universal", "-p", &proj]).code,
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
    } = json(&hub, &["move", "a/b/tdd", "c/d/tdd"]);
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

/// 末段不等：报 identity_immutable 并指路重入库，退出码 1、磁盘零改动
#[test]
fn move_rejects_tail_change_with_identity_immutable() {
    let (install, hub, _src) = hub_with_nested_tdd();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a/b/tdd", "c/b/other"]);
    assert_eq!(code, 1);
    let error = error_of(&out);
    assert_eq!(error["code"], serde_json::json!("identity_immutable"));
    let msg = error["message"].as_str().unwrap();
    assert!(msg.contains("重新 add"), "文案要指路重入库: {msg}");
    assert!(
        install.path().join("store/a/b/tdd/SKILL.md").is_file(),
        "磁盘零改动"
    );
}

/// 预检各失败路径：skill_not_found / already_exists / invalid_skill_key，均零落盘
#[test]
fn move_failure_paths_are_stable_and_disk_stays_intact() {
    let (install, hub, _src) = hub_with_nested_tdd();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a/b/nope", "c/b/nope"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("skill_not_found"));

    // 新键被占（先占住 c/b/tdd 位置的同类路径）
    std::fs::create_dir_all(install.path().join("store/c/b/tdd")).unwrap();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a/b/tdd", "c/b/tdd"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("already_exists"));
    std::fs::remove_dir_all(install.path().join("store/c/b/tdd")).unwrap();

    // 键非法：反斜杠
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a\\b\\tdd", "c/b/tdd"]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("invalid_skill_key")
    );
    // 新键非法同样拒绝
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a/b/tdd", "C:/tdd"]);
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("invalid_skill_key")
    );

    assert!(
        install.path().join("store/a/b/tdd/SKILL.md").is_file(),
        "磁盘零改动"
    );
}

/// 归档面拒绝（文案指路 restore）；builtin 前缀可迁移；根级目标键可迁移
#[test]
fn move_gates_archive_builtin_and_root_level() {
    // 归档面：archive 后 move 被拒，文案指 restore
    let (install, hub, _src) = hub_with_nested_tdd();
    assert_eq!(json(&hub, &["archive", "a/b/tdd"]).code, 0);
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a/b/tdd", "c/b/tdd"]);
    assert_eq!(code, 1);
    let error = error_of(&out);
    assert_eq!(error["code"], serde_json::json!("archived"));
    assert!(
        error["message"].as_str().unwrap().contains("restore"),
        "文案要指路 restore: {out}"
    );
    drop(install);

    // builtin 前缀：初始化的内置技能可迁移
    let install2 = tempfile::tempdir().unwrap();
    let hub2 = hub_root(install2.path());
    init_hub_at(&hub2);
    assert!(
        install2
            .path()
            .join("store/builtin/fuyao-skills-hub")
            .is_dir(),
        "init 应铺出内置技能"
    );
    let Run {
        stdout: out, code, ..
    } = json(
        &hub2,
        &["mv", "builtin/fuyao-skills-hub", "own/fuyao-skills-hub"],
    );
    assert_eq!(code, 0, "builtin 可迁移: {out}");
    assert!(
        install2
            .path()
            .join("store/own/fuyao-skills-hub/SKILL.md")
            .is_file()
    );
    drop(install2);

    // 根级目标键：摘掉全部前缀
    let (install3, hub3, _src3) = hub_with_nested_tdd();
    let Run {
        stdout: out, code, ..
    } = json(&hub3, &["mv", "a/b/tdd", "tdd"]);
    assert_eq!(code, 0, "根级键可迁移: {out}");
    assert!(install3.path().join("store/tdd/SKILL.md").is_file());
}

/// dry-run：动作清单齐全（挪目录 + 改登记 + 重链落点 + 提交），零落盘
#[test]
fn move_dry_run_lists_actions_without_touching_disk() {
    let (install, hub, _src) = hub_with_nested_tdd();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(&hub, &["use", "a/b/tdd", "-a", "universal", "-p", &proj]).code,
        0
    );
    let store_before =
        std::fs::read_to_string(install.path().join("store/a/b/tdd/SKILL.md")).unwrap();
    let reg_before = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();
    let junction_dest = project.path().join(".agents/skills/tdd");

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a/b/tdd", "c/b/tdd", "-n"]);
    assert_eq!(code, 0, "dry-run: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true));
    assert_eq!(d["from"], serde_json::json!("a/b/tdd"));
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

/// 预检期闸门：落点被换成普通目录时（登记 junction、实际不符），
/// 三闸在重链动手前就把整次 move 拒掉，磁盘零改动
#[test]
fn move_rejects_tampered_destination_before_touching_disk() {
    let (install, hub, _src) = hub_with_nested_tdd();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(&hub, &["use", "a/b/tdd", "-a", "universal", "-p", &proj]).code,
        0
    );
    let junction_dest = project.path().join(".agents/skills/tdd");
    let reg_before = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();

    // 把落点换成普通目录：三闸在重链前发现形态不符（登记 junction、实际普通目录），
    // 整体拒绝并回滚
    std::fs::remove_dir(&junction_dest).unwrap();
    std::fs::create_dir_all(&junction_dest).unwrap();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["mv", "a/b/tdd", "c/b/tdd"]);
    assert_eq!(code, 1, "换靶落点要被拒: {out}");
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("link_kind_mismatch"),
        "失败码: {out}"
    );

    // 回滚：store 目录回原位、登记簿原文还原
    assert!(
        install.path().join("store/a/b/tdd/SKILL.md").is_file(),
        "store 目录回原位"
    );
    assert!(!install.path().join("store/c/b/tdd").exists());
    assert_eq!(
        std::fs::read_to_string(install.path().join("projects.toml")).unwrap(),
        reg_before,
        "登记簿整文件还原"
    );
}
