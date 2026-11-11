//! Seam 2 集成测试（frontmatter 形状）：起真实 fsh 子进程，断言命令行一侧
//! 只认 name / description / source，metadata.version 完全忽略，解析报错
//! 原样透出。只断言外部行为，不测内部实现。

mod common;

use common::{Run, data_of, env_with_source, error_of, hub_root, init_hub_at, json};

/// 带 metadata 子块的 SKILL.md 原文；sub 是子块下的行（含缩进）
fn skill_md_with(sub: &str) -> String {
    format!("---\nname: demo\ndescription: 演示\nmetadata:\n{sub}---\n\n正文\n")
}

#[test]
fn version_field_is_fully_ignored_by_add() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    let store_md = install.path().join("store").join("demo").join("SKILL.md");
    init_hub_at(&hub);

    // 版本号写成任意垃圾值：不解析、不校验、不改文件
    let written = skill_md_with("  version: garbage\n");
    std::fs::write(src.join("SKILL.md"), &written).unwrap();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s]);
    assert_eq!(code, 0, "version 不解析就不校验: {out}");
    assert_eq!(data_of(&out)["key"], serde_json::json!("demo"));
    assert_eq!(
        std::fs::read_to_string(&store_md).unwrap(),
        written,
        "入库不得改动文件"
    );
}

#[test]
fn frontmatter_shape_errors_reach_the_cli_envelope() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    let source_md = src.join("SKILL.md");
    init_hub_at(&hub);

    // 非法 YAML：报错点名文件
    std::fs::write(
        &source_md,
        "---\nname: demo2\ndescription: 演示\n\tbad-indent: x\n---\n\n正文\n",
    )
    .unwrap();
    let skill_md = source_md.display().to_string();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s]);
    assert_eq!(code, 1, "非法 YAML 应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("invalid_source"));
    let msg = err["message"].as_str().unwrap();
    assert!(msg.contains(&skill_md), "报错要点名文件: {msg}");
    assert!(msg.contains("解析失败"), "报错要点名解析失败: {msg}");

    // name 不合规：报错点名字段
    std::fs::write(
        &source_md,
        "---\nname: Demo\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s]);
    assert_eq!(code, 1, "非法 name 应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("invalid_source"));
    let msg = err["message"].as_str().unwrap();
    assert!(msg.contains("name") && msg.contains(&skill_md), "{msg}");

    // description 超长：报错点名字段
    std::fs::write(
        &source_md,
        format!(
            "---\nname: demo\ndescription: {}\n---\n\n正文\n",
            "字".repeat(1025)
        ),
    )
    .unwrap();
    let Run {
        stdout: out, code, ..
    } = json(&hub, &["add", &src_s]);
    assert_eq!(code, 1, "超长 description 应被拒: {out}");
    assert_eq!(error_of(&out)["code"], serde_json::json!("invalid_source"));
}

/// 坏 frontmatter 的技能在列表里降级为空 description，不中断整页
#[test]
fn list_degrades_rows_with_broken_frontmatter() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    init_hub_at(&hub);
    assert_eq!(json(&hub, &["add", &src_s]).code, 0);
    let work = install.path().join("store").join("demo").join("SKILL.md");
    std::fs::write(&work, "没有 frontmatter 的正文\n").unwrap();

    let Run {
        stdout: out, code, ..
    } = json(&hub, &["list"]);
    assert_eq!(code, 0, "坏 frontmatter 不中断列表: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    let demo = rows.iter().find(|r| r["key"] == "demo").unwrap();
    assert_eq!(demo["description"], serde_json::json!(""));
}
