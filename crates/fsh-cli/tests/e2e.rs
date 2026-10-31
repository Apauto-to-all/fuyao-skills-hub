//! M1 端到端验收（dogfood）：仓库自带的伴生 skill manage-skills 当第一个
//! 入库对象，真实子进程跑通「入库 → 分配 → 应用 → 查状态 → 漂移」全链路。

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// 起一次 fsh，返回 (stdout, 退出码)
fn fsh(args: &[&str]) -> (String, i32) {
    let out = Command::new(env!("CARGO_BIN_EXE_fsh"))
        .args(args)
        .output()
        .expect("起 fsh 子进程失败");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

/// 解析成功信封并返回 data 节点
fn data_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(true), "信封: {v}");
    v["data"].clone()
}

#[test]
fn m1_dogfood_lifecycle_end_to_end() {
    let install = tempfile::tempdir().unwrap();
    let hub = install.path().join("data").display().to_string();
    // 伴生 skill 源 = 本仓库 skills/manage-skills（自己吃自己的狗粮）
    let skill_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/manage-skills");
    let src = skill_src.display().to_string();

    // ① 初始化 hub
    let (out, code) = fsh(&["--json", "--hub", &hub, "init"]);
    assert_eq!(code, 0, "init: {out}");

    // ② 入库伴生 skill 本体：首版 0.1.0 一步到位
    let (out, code) = fsh(&["--json", "--hub", &hub, "add", &src]);
    assert_eq!(code, 0, "add: {out}");
    let d = data_of(&out);
    assert_eq!(d["skill"], serde_json::json!("manage-skills"));
    assert_eq!(d["version"], serde_json::json!("0.1.0"));

    // ③ 项目声明：手写 skills.toml 要这个 skill
    let project = tempfile::tempdir().unwrap();
    std::fs::write(
        project.path().join("skills.toml"),
        "[[skills]]\nname = \"manage-skills\"\nversion = \"0.1.0\"\ntarget = \"claude-code\"\n",
    )
    .unwrap();
    let proj = project.path().display().to_string();

    // ④ plan 预览一条 Install → apply 铺链 → 链接落点真有文件
    let (out, code) = fsh(&["--json", "--hub", &hub, "plan", &proj]);
    assert_eq!(code, 0, "plan: {out}");
    assert_eq!(data_of(&out)["changes"].as_array().unwrap().len(), 1);

    let (out, code) = fsh(&["--json", "--hub", &hub, "apply", &proj]);
    assert_eq!(code, 0, "apply: {out}");
    let link = project.path().join(".fsh/skills/manage-skills/SKILL.md");
    assert!(link.is_file(), "链接落点应有 SKILL.md: {}", link.display());

    // ⑤ status 干净（退出码 0）→ 制造漂移（删链接）→ 漂移（退出码 2）
    let (out, code) = fsh(&["--json", "--hub", &hub, "status", &proj]);
    assert_eq!(code, 0, "status 应干净: {out}");
    assert_eq!(data_of(&out)["clean"], serde_json::json!(true));

    std::fs::remove_dir_all(project.path().join(".fsh/skills/manage-skills")).unwrap();
    let (out, code) = fsh(&["--json", "--hub", &hub, "status", &proj]);
    assert_eq!(code, 2, "缺链应漂移: {out}");
    assert_eq!(data_of(&out)["clean"], serde_json::json!(false));
}
