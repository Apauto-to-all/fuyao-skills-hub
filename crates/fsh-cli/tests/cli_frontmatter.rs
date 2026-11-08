//! Seam 2 集成测试（frontmatter 的 metadata 形状）：起真实 fsh 子进程，断言命令行
//! 一侧按 `metadata` 取号、如实列出写回那一步、把解析报错原样透出、带出解析失败
//! 原因、按逗号分隔的标签过滤。只断言外部行为，不测内部实现。

mod common;

use serde_json::Value;

use common::{data_of, env_with_source, error_of, fsh, hub_root};

/// 带 metadata 子块的 SKILL.md 原文；sub 是子块下的行（含缩进）
fn skill_md_with(sub: &str) -> String {
    format!("---\nname: demo\ndescription: 演示\nmetadata:\n{sub}---\n\n正文\n")
}

/// 字段在信封里显式为 null：契约要的是 null 而不是字段缺席，两者必须分辨
fn assert_present_null(row: &Value, field: &str) {
    assert!(
        row.get(field).is_some_and(|v| v.is_null()),
        "{field} 应在且为 null: {row}"
    );
}

#[test]
fn release_reads_number_from_metadata_and_only_moves_forward() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    // 工作副本按层拼：报错里的文件路径由 core 拼出，要比对就得同形
    let work = install.path().join("store").join("demo").join("SKILL.md");

    // 源里没号：补出的首版号落进 metadata.version，入库结果同时给出这个号
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
    assert_eq!(code, 0, "add: {out}");
    assert_eq!(data_of(&out)["version"], serde_json::json!("0.1.0"));
    let landed = std::fs::read_to_string(&work).unwrap();
    assert!(
        landed.contains("metadata:\n  version: \"0.1.0\""),
        "首版号应写进 metadata.version: {landed}"
    );

    // 编辑面把号写成 0.2.0：预演与实跑都按文件里的号走
    std::fs::write(&work, skill_md_with("  version: \"0.2.0\"\n")).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "demo", "--dry-run"]);
    assert_eq!(code, 0, "release dry: {out}");
    let actions = data_of(&out)["actions"].as_array().unwrap().clone();
    assert!(
        actions
            .iter()
            .any(|a| a.as_str().unwrap() == "tag demo/v0.2.0"),
        "预演应列出按文件里的号打的 tag: {actions:?}"
    );
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "demo"]);
    assert_eq!(code, 0, "release: {out}");
    assert_eq!(data_of(&out)["version"], serde_json::json!("0.2.0"));

    // 号只能前进：同号重发被拒
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "demo"]);
    assert_eq!(code, 1, "同号重发应被拒: {out}");
    assert_eq!(
        error_of(&out)["code"],
        serde_json::json!("version_not_advancing")
    );

    // 落后的号同样被拒，且报错点出当前最新版本：此刻目标号是 0.1.0，
    // 报文里的 0.2.0 只可能来自当前版本
    std::fs::write(&work, skill_md_with("  version: \"0.1.0\"\n")).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "demo"]);
    assert_eq!(code, 1, "落后号应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("version_not_advancing"));
    let msg = err["message"].as_str().unwrap();
    assert!(
        msg.contains("0.2.0") && msg.contains("0.1.0"),
        "报错要点出当前最新版本与目标号: {msg}"
    );

    // 文件里没号时发版无处取号：失败信封点名字段与文件
    std::fs::write(&work, "---\nname: demo\ndescription: 演示\n---\n\n正文\n").unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "demo"]);
    assert_eq!(code, 1, "缺号应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("invalid_source"));
    let msg = err["message"].as_str().unwrap();
    assert!(
        msg.contains("metadata.version") && msg.contains("SKILL.md"),
        "报错要点名文件与字段: {msg}"
    );
}

#[test]
fn add_dry_run_lists_the_metadata_version_write_back() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    let source_md = src.join("SKILL.md");
    let store_md = install.path().join("store").join("demo").join("SKILL.md");
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);

    // 源里没号：预演如实列出补首版号那一步，且一字未落盘
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s, "--dry-run"]);
    assert_eq!(code, 0, "add dry: {out}");
    let actions = data_of(&out)["actions"].as_array().unwrap().clone();
    assert!(
        actions
            .iter()
            .any(|a| a.as_str().unwrap() == "写回 metadata.version = 0.1.0"),
        "预演要列出写回那一步: {actions:?}"
    );
    assert!(!store_md.exists(), "预演不得落盘");

    // 源里已写号：没有写回这步，入库结果就是源里的号，文件一个字节不改
    let written = skill_md_with("  version: \"1.0.0\"\n");
    std::fs::write(&source_md, &written).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s, "--dry-run"]);
    assert_eq!(code, 0, "add dry（已写号）: {out}");
    let actions = data_of(&out)["actions"].as_array().unwrap().clone();
    assert!(
        !actions.iter().any(|a| a.as_str().unwrap().contains("写回")),
        "源里已写号就没有写回这一步: {actions:?}"
    );
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
    assert_eq!(code, 0, "add: {out}");
    assert_eq!(data_of(&out)["version"], serde_json::json!("1.0.0"));
    assert_eq!(
        std::fs::read_to_string(&store_md).unwrap(),
        written,
        "源里已写号时入库不得改动文件"
    );
}

#[test]
fn frontmatter_shape_errors_reach_the_cli_envelope() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    let source_md = src.join("SKILL.md");
    let skill_md = source_md.display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);

    // 顶层错位：fsh 自有字段平铺在顶层 → 点名文件、字段与正确位置
    std::fs::write(
        &source_md,
        "---\nname: demo\ndescription: 演示\nversion: \"0.2.0\"\n---\n\n正文\n",
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
    assert_eq!(code, 1, "顶层 version 应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("invalid_source"));
    let msg = err["message"].as_str().unwrap();
    assert!(msg.contains(&skill_md), "报错要点名文件: {msg}");
    assert!(
        msg.contains("version") && msg.contains("metadata"),
        "报错要点名字段与正确位置: {msg}"
    );

    // name 不合规：报错点名字段
    std::fs::write(
        &source_md,
        "---\nname: Demo\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
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
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src_s]);
    assert_eq!(code, 1, "超长 description 应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("invalid_source"));
    assert!(
        err["message"].as_str().unwrap().contains("description"),
        "{err}"
    );

    // 发版一侧同一套判据：工作副本顶层错位同样点名文件与字段
    std::fs::write(
        &source_md,
        "---\nname: demo\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    assert_eq!(fsh(&["--json", "--hub", &hub, "add", &src_s]).1, 0);
    let work = install.path().join("store").join("demo").join("SKILL.md");
    std::fs::write(
        &work,
        "---\nname: demo\ndescription: 演示\nversion: \"0.2.0\"\n---\n\n正文\n",
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "release", "demo"]);
    assert_eq!(code, 1, "发版遇顶层错位应被拒: {out}");
    let err = error_of(&out);
    assert_eq!(err["code"], serde_json::json!("invalid_source"));
    let msg = err["message"].as_str().unwrap();
    let named = work.display().to_string();
    assert!(msg.contains(&named), "报错要点名文件: {msg}");
    assert!(
        msg.contains("version") && msg.contains("metadata"),
        "报错要点名字段与正确位置: {msg}"
    );
}

#[test]
fn show_carries_the_parse_error_while_list_degrades() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    let src_s = src.display().to_string();
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);
    std::fs::write(
        src.join("SKILL.md"),
        skill_md_with("  version: \"1.0.0\"\n  source: owner/repo\n  tags: \"tdd, office\"\n"),
    )
    .unwrap();
    assert_eq!(fsh(&["--json", "--hub", &hub, "add", &src_s]).1, 0);

    // 解析成功：原因字段在且为 null（与同结构其它可选字段同形，不是字段缺席）
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 0, "show: {out}");
    let d = data_of(&out);
    assert_present_null(&d, "frontmatter_error");
    assert_eq!(d["frontmatter"]["source"], serde_json::json!("owner/repo"));
    assert_eq!(
        d["frontmatter"]["tags"],
        serde_json::json!(["tdd", "office"])
    );

    // 工作副本被改坏：查询面照旧成功，失败原因带出来并点名文件与字段
    let work = install.path().join("store").join("demo").join("SKILL.md");
    std::fs::write(
        &work,
        "---\nname: demo\ndescription: 演示\ntags: \"tdd\"\n---\n\n正文\n",
    )
    .unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "show", "demo"]);
    assert_eq!(code, 0, "坏 frontmatter 不该让 show 失败: {out}");
    let d = data_of(&out);
    assert_present_null(&d, "frontmatter");
    let reason = d["frontmatter_error"]
        .as_str()
        .expect("解析失败应带出原因")
        .to_string();
    assert!(
        reason.contains("store/demo/SKILL.md") && reason.contains("tags"),
        "原因要点名文件与字段: {reason}"
    );
    assert!(d["content"].as_str().unwrap().contains("正文"), "{d}");

    // list 照旧降级：坏文件只表现为元数据为空，不中断整页
    let (out, code) = fsh(&["--json", "--hub", &hub, "list"]);
    assert_eq!(code, 0, "list: {out}");
    let rows = data_of(&out)["skills"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["key"], serde_json::json!("demo"));
    assert_eq!(rows[0]["latest_version"], serde_json::json!("1.0.0"));
    assert_present_null(&rows[0], "source");
    assert_eq!(rows[0]["tags"], serde_json::json!([]));
}

#[test]
fn list_filters_by_comma_separated_metadata_tags() {
    let (install, src) = env_with_source();
    let hub = hub_root(install.path());
    assert_eq!(fsh(&["--json", "--hub", &hub, "init"]).1, 0);

    // 两个技能各带标签，其中一个的写法带多余空格与空项
    std::fs::write(
        src.join("SKILL.md"),
        skill_md_with("  tags: \" tdd ,, office \"\n"),
    )
    .unwrap();
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &src.display().to_string()]).1,
        0
    );
    let other = install.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("SKILL.md"),
        "---\nname: other\ndescription: 另一个\nmetadata:\n  tags: \"office\"\n---\n\n正文\n",
    )
    .unwrap();
    assert_eq!(
        fsh(&["--json", "--hub", &hub, "add", &other.display().to_string()]).1,
        0
    );

    // 命中键按键排序后比对，不依赖清单的排列顺序
    let hit = |tag: &str| -> Vec<String> {
        let (out, code) = fsh(&["--json", "--hub", &hub, "list", "--tag", tag]);
        assert_eq!(code, 0, "list --tag {tag}: {out}");
        let mut keys: Vec<String> = data_of(&out)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["key"].as_str().unwrap().to_string())
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(hit("tdd"), vec!["demo".to_string()]);
    assert_eq!(hit("office"), vec!["demo".to_string(), "other".to_string()]);
    // 不命中：整页照出、不报错
    assert!(hit("tddx").is_empty(), "标签要整项比对，前缀不算命中");
}
