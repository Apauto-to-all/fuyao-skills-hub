//! 指定链接方式：参数取值、盘上形态、登记簿记录、预演形状与失败零残留。

mod common;

use std::fs;
use std::path::Path;

use common::{
    data_of, env_with_source, error_of, has_step, hub_with_a_set, initialized_hub, json, text,
};
use serde_json::json as value;

/// 落点是不是真目录：符号链接与目录联接在 Rust 侧都表现为 symlink
fn is_real_dir(dest: &Path) -> bool {
    let meta = fs::symlink_metadata(dest).expect("落点应存在");
    !meta.file_type().is_symlink() && meta.is_dir()
}

/// 落点是不是 reparse point（符号链接或目录联接）
fn is_reparse_point(dest: &Path) -> bool {
    fs::symlink_metadata(dest)
        .expect("落点应存在")
        .file_type()
        .is_symlink()
}

/// 登记簿原文：一次都没写过就是空串
fn registry(install: &Path) -> String {
    fs::read_to_string(install.join("projects.toml")).unwrap_or_default()
}

/// 指定 copy：每个 agent 的落点都是真目录、内容读得到，登记簿与 list 都记 copy
#[test]
fn copy_lands_a_real_directory_for_every_agent() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    let r = json(
        &hub,
        &[
            "use",
            "--skill",
            "demo",
            "-a",
            "universal,dsh",
            "-p",
            &proj,
            "-l",
            "copy",
        ],
    );
    assert_eq!(r.code, 0, "use --link copy: {}", r.stdout);

    for name in [".agents/skills/demo", ".dsh/skills/demo"] {
        let dest = project.path().join(name);
        assert!(is_real_dir(&dest), "{name} 应是真目录");
        assert!(dest.join("SKILL.md").is_file(), "{name} 应能读到内容");
    }
    let reg = registry(install.path());
    assert!(reg.contains("link = \"copy\""), "登记簿: {reg}");

    let listed = text(&hub, &["list"]);
    assert_eq!(listed.code, 0, "{}", listed.stdout);
    assert!(
        listed.stdout.contains("copy"),
        "使用清单应显示 copy: {}",
        listed.stdout
    );
}

/// 指定 junction：落点是 reparse point，穿透后读到工作副本
#[test]
fn junction_lands_a_reparse_point() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    let r = json(
        &hub,
        &[
            "use", "--skill", "demo", "-a", "dsh", "-p", &proj, "-l", "junction",
        ],
    );
    assert_eq!(r.code, 0, "use --link junction: {}", r.stdout);

    let dest = project.path().join(".dsh/skills/demo");
    assert!(
        is_reparse_point(&dest),
        "指定目录联接就该建成 reparse point"
    );
    assert!(dest.join("SKILL.md").is_file(), "穿透后应读到工作副本");
    let reg = registry(install.path());
    assert!(reg.contains("link = \"junction\""), "登记簿: {reg}");
}

/// 指定符号链接：本机建得成就建成符号链接，建不成就报 link_failed 且不留残留。
/// 走哪一支取决于跑测试的机器有没有开发者模式，两支都断言
#[test]
fn symlink_is_built_or_refused_without_residue() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let dest = project.path().join(".dsh/skills/demo");

    let r = json(
        &hub,
        &[
            "use", "--skill", "demo", "-a", "dsh", "-p", &proj, "-l", "symlink",
        ],
    );
    if r.code == 0 {
        assert!(
            is_reparse_point(&dest),
            "指定符号链接就该建成 reparse point"
        );
        assert!(dest.join("SKILL.md").is_file());
        let reg = registry(install.path());
        assert!(reg.contains("link = \"symlink\""), "登记簿: {reg}");
    } else {
        let e = error_of(&r.stdout);
        assert_eq!(e["code"], value!("link_failed"), "{e}");
        let message = e["message"].as_str().unwrap();
        assert!(message.contains("符号链接"), "报错要点名方式: {message}");
        assert!(!dest.exists(), "建不出来要撤干净，不留半截落点");
        assert!(!registry(install.path()).contains("demo"), "失败不写登记");
    }
}

/// 不给 --link：预演与信封都保持原样，不带任何链接方式
#[test]
fn auto_keeps_the_old_preview_shape() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let args = ["use", "--skill", "demo", "-a", "dsh", "-p", &proj, "-n"];

    let t = text(&hub, &args);
    assert_eq!(t.code, 0, "{}", t.stdout);
    let line = t
        .stdout
        .lines()
        .find(|l| l.contains(" -> "))
        .expect("预演应有建落点的一行");
    assert!(line.starts_with("- link "), "{line}");
    assert!(!line.contains("link("), "不指定时不该带方式: {line}");

    let d = data_of(&json(&hub, &args).stdout);
    let actions = d["actions"].as_array().expect("预演应有动作清单");
    let step = actions
        .iter()
        .find(|s| s["action"] == value!("link"))
        .expect("应有建落点步骤");
    assert!(
        step.get("link").is_none(),
        "不指定时信封里不该有 link: {step}"
    );
}

/// 指定了方式：预演文本把方式缀在动作词后，信封里也带上同一个词
#[test]
fn preview_shows_the_specified_kind() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let args = [
        "use", "--skill", "demo", "-a", "dsh", "-p", &proj, "-l", "copy", "-n",
    ];

    let t = text(&hub, &args);
    assert_eq!(t.code, 0, "{}", t.stdout);
    assert!(t.stdout.contains("link(copy) "), "{}", t.stdout);

    let d = data_of(&json(&hub, &args).stdout);
    assert!(
        has_step(&d["actions"], value!({"action": "link", "link": "copy"})),
        "{}",
        d["actions"]
    );
    // 预演不落盘
    assert!(
        !project.path().join(".dsh/skills/demo").exists(),
        "预演不该建出落点"
    );
}

/// 一次调用一个方式，覆盖该次点到的全部技能
#[test]
fn one_call_pins_every_skill_of_the_batch() {
    let (install, hub, _src_root) = hub_with_a_set();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    let r = json(
        &hub,
        &[
            "use", "--prefix", "k1/k2", "--all", "-a", "dsh", "-p", &proj, "-l", "copy",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stdout);

    for name in ["tdd", "db", "ops"] {
        let dest = project.path().join(".dsh/skills").join(name);
        assert!(is_real_dir(&dest), "{name} 应是真目录");
    }
    let reg = registry(install.path());
    assert_eq!(
        reg.matches("link = \"copy\"").count(),
        3,
        "三条记录都该记 copy: {reg}"
    );
}

/// 取值只认三个词：别的词由参数层拒绝，消息列出合法取值，且不落盘
#[test]
fn unknown_kind_is_refused_by_the_parser() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    let r = json(
        &hub,
        &[
            "use", "--skill", "demo", "-a", "dsh", "-p", &proj, "-l", "soft",
        ],
    );
    assert_eq!(r.code, 1, "参数错不该成功: {}", r.stdout);
    let e = error_of(&r.stdout);
    let message = e["message"].as_str().unwrap();
    for word in ["symlink", "junction", "copy"] {
        assert!(
            message.contains(word),
            "报错要列出合法取值 {word}: {message}"
        );
    }
    assert!(
        !project.path().join(".dsh/skills/demo").exists(),
        "参数错不落盘"
    );
    assert!(!registry(install.path()).contains("demo"), "参数错不写登记");
}

/// 已登记的技能再带 --link：仍按 already_registered 跳过，盘上与登记簿都不动
#[test]
fn already_registered_stays_skipped_even_with_a_kind() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    let dest = project.path().join(".dsh/skills/demo");

    assert_eq!(
        json(&hub, &["use", "--skill", "demo", "-a", "dsh", "-p", &proj]).code,
        0,
        "首次铺装应成功"
    );
    let before = registry(install.path());
    let pinned = is_reparse_point(&dest);

    let r = json(
        &hub,
        &[
            "use", "--skill", "demo", "-a", "dsh", "-p", &proj, "-l", "copy",
        ],
    );
    assert_eq!(r.code, 1, "什么都没做时退出码 1: {}", r.stdout);
    let d = data_of(&r.stdout);
    assert_eq!(d["skipped"][0]["code"], value!("already_registered"), "{d}");
    assert_eq!(registry(install.path()), before, "登记簿不该动");
    assert_eq!(is_reparse_point(&dest), pinned, "落点形态不该动");
}

/// 帮助里查得到三个合法取值，并写明指定后不降级
#[test]
fn help_lists_the_three_values() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);

    let r = text(&hub, &["use", "-h"]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    for word in ["symlink", "junction", "copy", "自动降级", "不降级"] {
        assert!(r.stdout.contains(word), "帮助应提到 {word}: {}", r.stdout);
    }
}

/// 实跑输出回归：不给 --link 时仍是一句完成陈述，不冒出链接方式的痕迹
#[test]
fn auto_run_reports_plain_completion() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();

    let r = text(&hub, &["use", "--skill", "demo", "-a", "dsh", "-p", &proj]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert_eq!(r.stdout.trim_end(), "使用完成：demo", "{}", r.stdout);
    assert!(
        !r.stdout.contains("link("),
        "实跑输出不该带链接方式: {}",
        r.stdout
    );
}

/// 整批回滚：拷不动批里最后一个技能，前面已建的两个落点一并撤净，登记簿不留记录。
/// 独占打开走 Windows 的 `share_mode(0)`：别的进程连读都读不到，拷贝必在那一处失败
#[test]
#[cfg(windows)]
fn a_failed_copy_rolls_the_whole_batch_back() {
    use std::os::windows::fs::OpenOptionsExt;

    let (install, hub, _src_root) = hub_with_a_set();
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    // 落点按技能键排序铺：db、ops 先建成，tdd 拷到一半失败
    let blocked = install.path().join("store/k1/k2/tdd/SKILL.md");
    let _held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&blocked)
        .expect("独占打开待失败的技能文件");

    let r = json(
        &hub,
        &[
            "use", "--prefix", "k1/k2", "--all", "-a", "dsh", "-p", &proj, "-l", "copy",
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stdout);
    let e = error_of(&r.stdout);
    assert_eq!(e["code"], value!("link_failed"), "{e}");
    for name in ["db", "ops", "tdd"] {
        let dest = project.path().join(".dsh/skills").join(name);
        assert!(!dest.exists(), "{name} 的落点应被回滚撤净");
    }
    let reg = registry(install.path());
    assert!(!reg.contains("k1/k2"), "失败的整批不该留登记: {reg}");
}

/// copy 是快照：手改拷贝后撤回被闸拦住，落点留着；加 -f 才撤得掉
#[test]
fn forced_unuse_removes_a_modified_copy() {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(
            &hub,
            &[
                "use", "--skill", "demo", "-a", "dsh", "-p", &proj, "-l", "copy"
            ]
        )
        .code,
        0,
        "铺 copy 应成功"
    );
    let dest = project.path().join(".dsh/skills/demo");
    fs::write(dest.join("SKILL.md"), "手改内容").unwrap();

    let r = json(
        &hub,
        &["unuse", "--skill", "demo", "-a", "dsh", "-p", &proj],
    );
    assert_eq!(r.code, 1, "{}", r.stdout);
    let e = error_of(&r.stdout);
    assert_eq!(e["code"], value!("modified_copy"), "{e}");
    assert!(dest.is_dir(), "被闸拦住时落点要留着");

    let r = json(
        &hub,
        &["unuse", "--skill", "demo", "-a", "dsh", "-p", &proj, "-f"],
    );
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(!dest.exists(), "加 -f 后撤回成功");
}
