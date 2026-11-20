//! 键迁移（move/mv）的用法错误与边界：形状拒绝、预检失败、闸门拒绝与回滚。
//! 全部经真实子进程与盘面观察，失败的调用磁盘零改动。

mod common;

use common::{Run, error_of, hub_root, hub_with_nested_tdd, init_hub_at, json};

/// 零兼容的形状：技能名恒为一段（带斜杠报 invalid_skill_key）、新前缀只收前缀；
/// 缺 --to、缺 --skill、--skill 给多个一律用法错误。全部零落盘
#[test]
fn move_shape_errors_stay_off_disk() {
    let (install, hub, _src) = hub_with_nested_tdd();

    // 名字里带斜杠是把分层混进了身份：报错教它写进 --prefix
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["mv", "--prefix", "a", "--skill", "b/tdd", "--to", "c"],
    );
    assert_eq!(code, 1);
    let error = error_of(&out);
    assert_eq!(error["code"], serde_json::json!("invalid_skill_key"));
    let msg = error["message"].as_str().unwrap();
    assert!(msg.contains("--prefix"), "文案要指路前缀: {msg}");

    // 新前缀按前缀的形状判：盘符冒号不成其为键段
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["mv", "--prefix", "a/b", "--skill", "tdd", "--to", "C:/x"],
    );
    assert_eq!(code, 1);
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("invalid_skill_key")
    );

    // 用法错误：缺 --to、缺 --skill、--skill 给多个、旧的两个位置参数
    for args in [
        vec!["mv", "--prefix", "a/b", "--skill", "tdd"],
        vec!["mv", "--prefix", "a/b", "--to", "c"],
        vec![
            "mv", "--prefix", "a/b", "--skill", "tdd", "--skill", "db", "--to", "c",
        ],
        vec!["mv", "a/b/tdd", "c/b/tdd"],
    ] {
        let Run {
            stdout: out, code, ..
        } = json(&hub, &args);
        assert_eq!(code, 1, "{args:?} 应按用法错误退出: {out}");
        assert_eq!(
            error_of(&out)["code"],
            serde_json::json!("usage"),
            "{args:?}: {out}"
        );
    }

    assert!(
        install.path().join("store/a/b/tdd/SKILL.md").is_file(),
        "磁盘零改动"
    );
}

/// 预检各失败路径：skill_not_found / already_exists，均零落盘
#[test]
fn move_failure_paths_are_stable_and_disk_stays_intact() {
    let (install, hub, _src) = hub_with_nested_tdd();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["mv", "--prefix", "a/b", "--skill", "nope", "--to", "c/b"],
    );
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("skill_not_found"));

    // 新键被占（先占住 c/b/tdd 位置的同类路径）
    std::fs::create_dir_all(install.path().join("store/c/b/tdd")).unwrap();
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["mv", "--prefix", "a/b", "--skill", "tdd", "--to", "c/b"],
    );
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], serde_json::json!("already_exists"));
    std::fs::remove_dir_all(install.path().join("store/c/b/tdd")).unwrap();

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
    assert_eq!(
        json(&hub, &["archive", "--prefix", "a/b", "--skill", "tdd"]).code,
        0
    );
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["mv", "--prefix", "a/b", "--skill", "tdd", "--to", "c/b"],
    );
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
        &[
            "mv",
            "--prefix",
            "builtin",
            "--skill",
            "fuyao-skills-hub",
            "--to",
            "own",
        ],
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
    } = json(
        &hub3,
        &["mv", "--prefix", "a/b", "--skill", "tdd", "--to", ""],
    );
    assert_eq!(code, 0, "根级键可迁移: {out}");
    assert!(install3.path().join("store/tdd/SKILL.md").is_file());
}

/// 预检期闸门：落点被换成普通目录时（登记 junction、实际不符），
/// 三闸在重链动手前就把整次 move 拒掉，磁盘零改动
#[test]
fn move_rejects_tampered_destination_before_touching_disk() {
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
    let junction_dest = project.path().join(".agents/skills/tdd");
    let reg_before = std::fs::read_to_string(install.path().join("projects.toml")).unwrap();

    // 把落点换成普通目录：三闸在重链前发现形态不符（登记 junction、实际普通目录），
    // 整体拒绝并回滚
    std::fs::remove_dir(&junction_dest).unwrap();
    std::fs::create_dir_all(&junction_dest).unwrap();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &["mv", "--prefix", "a/b", "--skill", "tdd", "--to", "c/b"],
    );
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
