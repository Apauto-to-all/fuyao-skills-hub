//! fsh search 集成测试：全部走真实子进程，只断言 stdout 形态与退出码。
//! 夹具复用 common 的真 init hub，再铺检索内容（store 内技能与散件、
//! agents 配置与根级散件），命中集恒为三条 needle。

mod common;

use std::fs;

use serde_json::json;

use common::{Run, data_of, error_of, hub_root, text};

/// 造检索夹具：demo 技能（入库自 common）之上补 notes、分组技能 beta、
/// agents 配置与白名单外文件；needle（smart-case 全小写）恒命中三条
fn fixture() -> (tempfile::TempDir, String, Vec<&'static str>) {
    let (install, src) = common::env_with_source();
    let hub = common::initialized_hub(&install, &src);
    let root = install.path();
    fs::write(root.join("store/demo/notes.md"), "line1\nNEEDLE line two\n").unwrap();
    fs::create_dir_all(root.join("store/group/beta")).unwrap();
    fs::write(
        root.join("store/group/beta/SKILL.md"),
        "---\nname: beta\ndescription: beta skill\n---\n\nbeta 中文针 needle 中文\n后一行 tail\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("agents")).unwrap();
    fs::write(
        root.join("agents/claude.toml"),
        "model = \"needle-model\"\n",
    )
    .unwrap();
    fs::write(root.join("stray.txt"), "stray needle\n").unwrap();
    let expected = vec![
        "agents/claude.toml:1:model = \"needle-model\"",
        "store/demo/notes.md:2:NEEDLE line two",
        "store/group/beta/SKILL.md:6:beta 中文针 needle 中文",
    ];
    (install, hub, expected)
}

/// 文本形态：path:line:text、正斜杠、无色、恒定排序；白名单外文件不出现
#[test]
fn text_mode_prints_hit_lines_in_stable_order() {
    let (_install, hub, expected) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().collect::<Vec<_>>(), expected);
}

/// JSON 信封：hits/total/truncated 与每条命中的 key 投影、spans、行文本
#[test]
fn json_mode_envelope_has_hits_keys_and_spans() {
    let (_install, hub, _) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["--json", "search", "needle"]);
    assert_eq!(code, 0);
    let data = data_of(&out);
    assert_eq!(data["total"], json!(3));
    assert_eq!(data["truncated"], json!(false));
    let hits = data["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 3);
    // 命中不含 context 键：上下文行只存在于文本模式的展开输出
    assert!(hits.iter().all(|h| h.get("context").is_none()));
    // store 外命中省略键，store 内命中带键
    assert!(hits[0].get("key").is_none());
    assert_eq!(hits[0]["path"], json!("agents/claude.toml"));
    assert_eq!(hits[0]["line"], json!(1));
    assert_eq!(hits[1]["key"], json!("demo"));
    assert_eq!(hits[2]["key"], json!("group/beta"));
    // spans 是行内字符偏移：中文前缀 9 个字符后命中 needle（15-9=6）
    assert_eq!(hits[2]["spans"], json!([[9, 15]]));
    assert_eq!(hits[2]["text"], json!("beta 中文针 needle 中文"));
}

/// -l 列命中文件、-c 每文件计数，形态与排序恒定
#[test]
fn dash_l_and_dash_c_shape_file_views() {
    let (_install, hub, _) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-l"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            "agents/claude.toml",
            "store/demo/notes.md",
            "store/group/beta/SKILL.md",
        ]
    );
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-c"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            "agents/claude.toml:1",
            "store/demo/notes.md:1",
            "store/group/beta/SKILL.md:1",
        ]
    );
}

/// 上下文 flag：仅文本模式；命中行冒号分隔、上下文行 - 分隔、不连续组 --
#[test]
fn context_flags_print_context_lines_text_only() {
    let (_install, hub, _) = fixture();
    // -C 1：每个命中前后各一行，文件边界裁掉
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-C", "1"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            "agents/claude.toml:1:model = \"needle-model\"",
            "--",
            "store/demo/notes.md:1-:line1",
            "store/demo/notes.md:2:NEEDLE line two",
            "--",
            "store/group/beta/SKILL.md:5-:",
            "store/group/beta/SKILL.md:6:beta 中文针 needle 中文",
            "store/group/beta/SKILL.md:7-:后一行 tail",
        ]
    );
    // -B 1 只带前一行
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-B", "1"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.contains(&"store/demo/notes.md:1-:line1"));
    assert!(!lines.iter().any(|l| l.ends_with("后一行 tail")));
    // -A 1 只带后一行
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-A", "1"]);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.contains(&"store/group/beta/SKILL.md:7-:后一行 tail"));
    assert!(!lines.contains(&"store/demo/notes.md:1-:line1"));
}

/// 重叠上下文去重：同文件相邻命中的上下文区间重叠时每行只出一次，
/// 既被上文带出又自身命中的行按命中行出
#[test]
fn overlapping_context_lines_print_once() {
    let (install, hub, _) = fixture();
    fs::write(
        install.path().join("store/demo/multi.md"),
        "l1\nl2 needle\nl3\nl4 needle\nl5\nl6 needle\nl7\n",
    )
    .unwrap();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-g", "**/multi.md", "-C", "1"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec![
            "store/demo/multi.md:1-:l1",
            "store/demo/multi.md:2:l2 needle",
            "store/demo/multi.md:3-:l3",
            "store/demo/multi.md:4:l4 needle",
            "store/demo/multi.md:5-:l5",
            "store/demo/multi.md:6:l6 needle",
            "store/demo/multi.md:7-:l7",
        ]
    );
}

/// -A/-B/-C 互斥、且与 --json 同给一律用法冲突（信封码 usage、退出 1）
#[test]
fn context_flags_conflict_with_json_and_each_other() {
    let (_install, hub, _) = fixture();
    for flag in ["-C", "-A", "-B"] {
        let Run {
            stdout: out, code, ..
        } = text(&hub, &["--json", "search", "needle", flag, "1"]);
        assert_eq!(code, 1, "flag {flag} 与 --json 未冲突");
        assert_eq!(error_of(&out)["code"], json!("usage"));
    }
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-A", "1", "-B", "1"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], json!("usage"));
    // -l 与 -c 互斥
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-l", "-c"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], json!("usage"));
}

/// smart-case 缺省（含大写即敏感）与 -i 强制忽略大小写
#[test]
fn smart_case_default_and_dash_i_override() {
    let (_install, hub, _) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "NEEDLE"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec!["store/demo/notes.md:2:NEEDLE line two"]
    );
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "NEEDLE", "-i"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 3);
}

/// -F 字面量、-w 整词、-g glob 收窄各自生效
#[test]
fn literal_word_and_glob_narrow_matches() {
    let (install, hub, _) = fixture();
    fs::write(
        install.path().join("store/demo/lit.md"),
        "a.b dot\naxb cross\n",
    )
    .unwrap();
    // -F：元字符按字面量，只中 a.b 行
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "a.b", "-F"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec!["store/demo/lit.md:1:a.b dot"]
    );
    // 缺省按正则解释，axb 也命中
    let Run { stdout: out, .. } = text(&hub, &["search", "a.b"]);
    assert_eq!(out.lines().count(), 2);
    // -w 整词：need 不再命中 needle
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "need", "-w"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim_end(), "");
    // -g 只搜 SKILL.md
    let Run { stdout: out, .. } = text(&hub, &["search", "needle", "-g", "**/SKILL.md"]);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec!["store/group/beta/SKILL.md:6:beta 中文针 needle 中文"]
    );
}

/// --limit 截断：hits 被裁、total 精确、truncated 置位；文本模式只出截断后的行
#[test]
fn limit_truncates_and_json_reports_exact_total() {
    let (_install, hub, _) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["--json", "search", "needle", "--limit", "2"]);
    assert_eq!(code, 0);
    let data = data_of(&out);
    assert_eq!(data["hits"].as_array().unwrap().len(), 2);
    assert_eq!(data["total"], json!(3));
    assert_eq!(data["truncated"], json!(true));
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "--limit", "2"]);
    assert_eq!(code, 0);
    assert_eq!(out.lines().count(), 2);
}

/// 可选路径参数限定范围；越界与白名单外范围走失败信封
#[test]
fn path_argument_scopes_and_rejects_escape() {
    let (_install, hub, _) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "store/demo"]);
    assert_eq!(code, 0);
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec!["store/demo/notes.md:2:NEEDLE line two"]
    );
    for bad in ["..", "no-such-dir"] {
        let Run {
            stdout: out, code, ..
        } = text(&hub, &["--json", "search", "needle", bad]);
        assert_eq!(code, 1, "范围 {bad} 未被拒绝");
        assert_eq!(error_of(&out)["code"], json!("search_path_invalid"));
    }
}

/// 无命中退出 0；坏正则与 hub 未初始化走失败信封退出 1
#[test]
fn no_hits_exit_zero_and_failures_use_envelope() {
    let (_install, hub, _) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "绝不出现"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim_end(), "");
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["--json", "search", "绝不出现"]);
    assert_eq!(code, 0);
    let data = data_of(&out);
    assert_eq!(data["total"], json!(0));
    assert_eq!(data["hits"].as_array().unwrap().len(), 0);
    assert_eq!(data["truncated"], json!(false));

    let Run {
        stdout: out, code, ..
    } = text(&hub, &["--json", "search", "(unclosed"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], json!("search_pattern_invalid"));

    let bare = tempfile::tempdir().unwrap();
    let Run {
        stdout: out, code, ..
    } = text(&hub_root(bare.path()), &["--json", "search", "needle"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], json!("hub_not_initialized"));
}

/// 未定义的 flag 一律报 usage：检索 flag 面是封闭名单
#[test]
fn unknown_flags_are_rejected() {
    let (_install, hub, _) = fixture();
    let Run {
        stdout: out, code, ..
    } = text(&hub, &["search", "needle", "-m", "5"]);
    assert_eq!(code, 1);
    assert_eq!(error_of(&out)["code"], json!("usage"));
}
