//! 批量入库：两层发现、`--skill` / `--all` 筛选、未表态拒绝、坏技能整批不入、
//! 重键跳过、前缀并入、一次批量一条提交、预演逐条列出。全部经真实子进程与盘面观察。

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Run, data_of, error_of, init_hub, json, text};

/// 在 root 下造一个技能目录（frontmatter name 与目录名一致）
fn skill_at(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} 的说明\n---\n\n正文\n"),
    )
    .unwrap();
    dir
}

/// 造一个装着多个技能的源目录：tdd / db / ops 各一个，外加一个非技能子目录
fn pack() -> tempfile::TempDir {
    let src = tempfile::tempdir().unwrap();
    for name in ["tdd", "db", "ops"] {
        skill_at(src.path(), name);
    }
    fs::create_dir_all(src.path().join("docs")).unwrap();
    src
}

/// hub 仓库的机器提交消息，从旧到新
fn commit_subjects(hub: &str) -> Vec<String> {
    let Ok(repo) = fsh_core::git::open_hub(Path::new(hub)) else {
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

/// 指一个装着多个技能的目录并给 --all：一条命令把多个键入进同一前缀
#[test]
fn add_all_takes_every_skill_into_one_prefix() {
    let install = tempfile::tempdir().unwrap();
    let src = pack();
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--prefix", "k1/k2", "--all"]);
    assert_eq!(code, 0, "add --all: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/k2/db", "k1/k2/ops", "k1/k2/tdd"]),
        "{out}"
    );
    assert_eq!(d["skipped"], serde_json::json!([]), "{out}");
    for name in ["db", "ops", "tdd"] {
        assert!(
            install
                .path()
                .join("store/k1/k2")
                .join(name)
                .join("SKILL.md")
                .is_file(),
            "{name} 的工作副本应就位"
        );
    }
    assert!(
        !install.path().join("store/k1/k2/docs").exists(),
        "不含 SKILL.md 的子目录不入库"
    );

    // 一次批量只产生一条提交，消息含前缀与数量
    let history = commit_subjects(&hub);
    assert_eq!(
        history.last().unwrap(),
        "add(store): k1/k2 3 个技能",
        "整批一条提交: {history:?}"
    );
    assert_eq!(
        history.iter().filter(|m| m.starts_with("add(")).count(),
        1,
        "一次批量一条 add 提交: {history:?}"
    );
}

/// 只给 --skill 两个名字时只入这两个，其余不入
#[test]
fn add_skill_picks_only_the_named_ones() {
    let install = tempfile::tempdir().unwrap();
    let src = pack();
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "add", &src_s, "--prefix", "pack", "--skill", "tdd", "--skill", "ops",
        ],
    );
    assert_eq!(code, 0, "add --skill: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["pack/tdd", "pack/ops"]),
        "按点名顺序做掉: {out}"
    );
    assert!(install.path().join("store/pack/tdd").is_dir());
    assert!(install.path().join("store/pack/ops").is_dir());
    assert!(
        !install.path().join("store/pack/db").exists(),
        "没点名的技能不入库"
    );
}

/// 源目录自身含 SKILL.md：按单技能入库，子目录里的技能不被入库
#[test]
fn add_takes_a_single_skill_source_and_stops_there() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let src = skill_at(src_root.path(), "solo");
    skill_at(&src, "inner");
    let hub = init_hub(&install);
    let src_s = src.display().to_string();

    // 不给 --skill/--all 也成立：源里恰好一个技能
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s]);
    assert_eq!(code, 0, "单技能源: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["solo"]),
        "{out}"
    );
    assert!(install.path().join("store/solo/SKILL.md").is_file());
    assert!(
        !install.path().join("store/inner").exists(),
        "子目录里的技能不另立键"
    );

    // 单技能模式下 --all 等价于省略
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--prefix", "again", "--all"]);
    assert_eq!(code, 0, "单技能 --all: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["again/solo"]),
        "{out}"
    );
}

/// 三层布局：子目录的孙目录里还有 SKILL.md 时，孙目录不被入库
#[test]
fn add_never_enters_grandchild_directories() {
    let install = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    // outer 自身不是技能：它的子目录 inner 与孙目录 deep 都在第二层之下
    skill_at(&src.path().join("outer"), "inner");
    skill_at(&src.path().join("outer").join("inner"), "deep");
    // 一个正常的直接子技能，让这次批量有东西可入
    skill_at(src.path(), "plain");
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--prefix", "three", "--all"]);
    assert_eq!(code, 0, "三层布局: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["three/plain"]),
        "只认直接子目录: {out}"
    );
    assert!(
        !install.path().join("store/three/inner").exists(),
        "孙目录不另立键"
    );
    assert!(
        !install.path().join("store/three/deep").exists(),
        "更深层一律不进入"
    );
}

/// `.git` 与 `node_modules` 下的技能不被入库
#[test]
fn add_skips_dot_directories_and_node_modules() {
    let install = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    skill_at(&src.path().join(".git"), "hidden");
    skill_at(&src.path().join("node_modules"), "dep");
    skill_at(src.path(), "real");
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--all"]);
    assert_eq!(code, 0, "扫描跳过: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["real"]),
        "{out}"
    );
    assert!(!install.path().join("store/hidden").exists());
    assert!(!install.path().join("store/dep").exists());
}

/// 命中多个且未给 --skill/--all：拒绝并列出发现的技能清单
#[test]
fn add_refuses_an_unstated_pick_and_lists_the_skills() {
    let install = tempfile::tempdir().unwrap();
    let src = pack();
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--prefix", "k1"]);
    assert_eq!(code, 1, "未表态应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("ambiguous_source"), "{err}");
    let message = err["message"].as_str().unwrap();
    for name in ["tdd", "db", "ops"] {
        assert!(message.contains(name), "清单应列出 {name}: {message}");
    }
    assert!(
        message.contains("--skill") && message.contains("--all"),
        "报错要指出两条出路: {message}"
    );
    assert!(
        !install.path().join("store/k1").exists(),
        "拒绝时一个都不入"
    );
}

/// 坏 frontmatter 或名字与目录名不一致：整批不入库，全部坏项一次列出
#[test]
fn add_rejects_the_whole_batch_and_lists_every_bad_skill() {
    let install = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    skill_at(src.path(), "good");
    // 坏 frontmatter：name 不合规
    let bad_fm = src.path().join("bad-fm");
    fs::create_dir_all(&bad_fm).unwrap();
    fs::write(
        bad_fm.join("SKILL.md"),
        "---\nname: Bad\ndescription: 坏名字\n---\n",
    )
    .unwrap();
    // 名字与目录名不一致
    let mismatch = src.path().join("dir-x");
    fs::create_dir_all(&mismatch).unwrap();
    fs::write(
        mismatch.join("SKILL.md"),
        "---\nname: other\ndescription: 名不对\n---\n",
    )
    .unwrap();
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--all"]);
    assert_eq!(code, 1, "坏技能整批不入: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("invalid_source"), "{err}");
    let message = err["message"].as_str().unwrap();
    assert!(message.contains("bad-fm"), "坏项要点名: {message}");
    assert!(message.contains("dir-x"), "坏项一次列全: {message}");
    assert!(message.contains("other"), "名字不一致要说清: {message}");
    assert!(
        !install.path().join("store/good").exists(),
        "整批不入：好的那个也不进"
    );
    assert!(
        commit_subjects(&hub).iter().all(|m| !m.starts_with("add(")),
        "整批不入即无入库提交"
    );
}

/// 键已存在时进 skipped，其余照常入库，退出码 0
#[test]
fn add_skips_existing_keys_and_keeps_going() {
    let install = tempfile::tempdir().unwrap();
    let src = pack();
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();
    // 先单独入 db：第二次批量里它应进 skipped
    let db = src.path().join("db").display().to_string();
    assert_eq!(json(&hub, &["add", &db, "--prefix", "k1"]).code, 0);

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--prefix", "k1", "--all"]);
    assert_eq!(code, 0, "做成了就不算失败: {out}");
    let d = data_of(&out);
    assert_eq!(
        d["applied"],
        serde_json::json!(["k1/ops", "k1/tdd"]),
        "{out}"
    );
    assert_eq!(d["skipped"][0]["key"], serde_json::json!("k1/db"), "{out}");
    assert_eq!(
        d["skipped"][0]["code"],
        serde_json::json!("already_exists"),
        "{out}"
    );
    assert!(
        d["skipped"][0]["message"]
            .as_str()
            .unwrap()
            .contains("已存在"),
        "跳过项带原因: {out}"
    );

    // 一条都做不成时退出码 1
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--prefix", "k1", "--all"]);
    assert_eq!(code, 1, "只有跳过项时退出码 1: {out}");
    assert_eq!(data_of(&out)["applied"], serde_json::json!([]), "{out}");
}

/// 前缀已存在时照常并入，不报错
#[test]
fn add_merges_into_an_existing_prefix() {
    let install = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    let first = skill_at(src.path(), "tdd");
    assert_eq!(
        json(
            &hub,
            &["add", &first.display().to_string(), "--prefix", "k1/k2"]
        )
        .code,
        0
    );

    // 同一前缀再入一个：并进去，不报错
    let second = skill_at(src.path(), "db");
    let Run {
        stdout: out, code, ..
    } = json(
        &hub,
        &[
            "add",
            &second.display().to_string(),
            "--prefix",
            "k1/k2",
            "--all",
        ],
    );
    assert_eq!(code, 0, "前缀已存在照常并入: {out}");
    assert_eq!(
        data_of(&out)["applied"],
        serde_json::json!(["k1/k2/db"]),
        "{out}"
    );
    assert!(
        install.path().join("store/k1/k2/tdd").is_dir(),
        "先入的还在"
    );
    assert!(
        install.path().join("store/k1/k2/db").is_dir(),
        "后入的并进来"
    );
}

/// 预演：逐条列出动作与跳过项，盘上一字未动、不产生提交
#[test]
fn add_dry_run_lists_actions_and_skips_without_landing() {
    let install = tempfile::tempdir().unwrap();
    let src = pack();
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();
    let db = src.path().join("db").display().to_string();
    assert_eq!(json(&hub, &["add", &db, "--prefix", "k1"]).code, 0);
    let before = commit_subjects(&hub);

    // JSON 预演：逐条复制步 + 一条提交步 + 跳过清单
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--prefix", "k1", "--all", "-n"]);
    assert_eq!(code, 0, "预演: {out}");
    let d = data_of(&out);
    assert_eq!(d["dry_run"], serde_json::json!(true), "{out}");
    let actions = d["actions"].as_array().unwrap();
    for name in ["ops", "tdd"] {
        assert!(
            actions.iter().any(|a| a["action"] == "copy"
                && a["to"] == serde_json::json!(format!("store/k1/{name}"))),
            "预演列 {name} 的复制步: {actions:?}"
        );
    }
    assert_eq!(
        actions.iter().filter(|a| a["action"] == "commit").count(),
        1,
        "预演只列一条提交步: {actions:?}"
    );
    assert_eq!(d["skipped"][0]["key"], serde_json::json!("k1/db"), "{out}");

    // 文本预演：动作与跳过项逐行给出
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["add", &src_s, "--prefix", "k1", "--all", "-n"]);
    assert_eq!(code, 0, "预演文本: {out}");
    assert!(
        out.contains(&format!(
            "- copy {} -> store/k1/tdd",
            src.path().join("tdd").display()
        )),
        "{out}"
    );
    assert!(out.contains("- commit: add(store): k1 2 个技能"), "{out}");
    assert!(out.contains("- 跳过 k1/db（already_exists）"), "{out}");

    // 盘面零变化
    assert!(!install.path().join("store/k1/tdd").exists());
    assert_eq!(commit_subjects(&hub), before, "预演不产生提交");
}

/// `--all` 与 `--skill` 互斥：同时给按用法错误拒绝
#[test]
fn add_all_excludes_skill() {
    let install = tempfile::tempdir().unwrap();
    let src = pack();
    let hub = init_hub(&install);
    let src_s = src.path().display().to_string();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s, "--all", "--skill", "tdd"]);
    assert_eq!(code, 1, "两把选择器互斥: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("usage"), "{out}");
}
