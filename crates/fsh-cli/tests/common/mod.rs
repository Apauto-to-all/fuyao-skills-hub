//! 共享测试助手：起真实 fsh 子进程、统一信封解析与临时环境搭建。
//! 放在 tests/common/ 下不会被 cargo 当作独立测试二进制编译。

// 每个测试二进制只用得上其中一部分助手，未用到的不该报警
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// 一次 fsh 子进程的结果：成功看 stdout，失败还要看 stderr 与退出码
pub struct Run {
    /// 标准输出
    pub stdout: String,
    /// 标准错误：文本模式的错误行，以及每次命令完成留下的日志行
    pub stderr: String,
    /// 退出码；进程被信号终止时为 -1
    pub code: i32,
}

/// 起一次 fsh，原样传参，不补任何公共前缀
pub fn fsh(args: &[&str]) -> Run {
    run(&[], &[], args)
}

/// 解析成功信封，断言 ok:true 并返回 data 节点
pub fn data_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(true), "信封: {v}");
    v["data"].clone()
}

/// 解析失败信封，断言 ok:false 并返回 error 节点
pub fn error_of(stdout: &str) -> Value {
    let v: Value = serde_json::from_str(stdout).expect("stdout 应是一行 JSON");
    assert_eq!(v["ok"], serde_json::json!(false), "信封: {v}");
    v["error"].clone()
}

/// 起一次 fsh，附加环境变量（FSH_HUB 注入等），原样传参
pub fn fsh_env(env: &[(&str, &str)], args: &[&str]) -> Run {
    run(env, &[], args)
}

/// JSON 信封模式跑一条命令：`--json` 与 `--hub <hub>` 由这里补，
/// 用例只写自己关心的参数
pub fn json(hub: &str, args: &[&str]) -> Run {
    run(&[], &["--json", "--hub", hub], args)
}

/// 同 json，再附加环境变量：家目录与专有变量目录都经环境交给子进程
pub fn json_env(env: &[(&str, &str)], hub: &str, args: &[&str]) -> Run {
    run(env, &["--json", "--hub", hub], args)
}

/// 文本模式跑一条命令：只补 `--hub <hub>`，输出形态由命令自己定
pub fn text(hub: &str, args: &[&str]) -> Run {
    run(&[], &["--hub", hub], args)
}

/// 起子进程的唯一实现：前缀参数先于用例参数，环境变量整批注入
fn run(env: &[(&str, &str)], prefix: &[&str], args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_fsh"))
        .args(prefix)
        .args(args)
        .envs(env.iter().copied())
        .output()
        .expect("起 fsh 子进程失败");
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code().unwrap_or(-1),
    }
}

/// 建测试环境：临时 hub 根 + 一个待入库 skill 源目录
/// （目录名必须等于 frontmatter name，故用 demo；源目录位于 hub 根内，
/// 被忽略规则白名单挡在仓库之外，不影响库内容）
pub fn env_with_source() -> (tempfile::TempDir, PathBuf) {
    let install = tempfile::tempdir().unwrap();
    let src = install.path().join("demo");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: 演示\n---\n\n正文\n",
    )
    .unwrap();
    (install, src)
}

/// hub 根路径（临时目录本身即 hub 根）
pub fn hub_root(install: &Path) -> String {
    install.display().to_string()
}

/// 起一个已 init 的空 hub，返回 hub 根
pub fn init_hub(install: &tempfile::TempDir) -> String {
    let hub = hub_root(install.path());
    init_hub_at(&hub);
    hub
}

/// 就地把已算好的 hub 路径初始化：路径由调用方自己给出，失败即断言
pub fn init_hub_at(hub: &str) {
    assert_eq!(json(hub, &["init"]).code, 0, "init 夹具应成功");
}

/// 起一个已 init 且已入库该 skill 的 hub，返回 hub 根
pub fn initialized_hub(install: &tempfile::TempDir, src: &Path) -> String {
    let hub = init_hub(install);
    let src_s = src.display().to_string();
    assert_eq!(json(&hub, &["add", &src_s]).code, 0, "add 夹具应成功");
    hub
}

/// 建一个登记簿里被项目引用的 hub：用真实 use 造出引用，remove 与 rm 都按同一份登记簿拒绝。
/// 返回的 install 与 project 两个守卫由调用方持有到用例结束，避免临时目录提前清理
pub fn hub_with_referenced_demo() -> (tempfile::TempDir, tempfile::TempDir, String) {
    let (install, src) = env_with_source();
    let project = tempfile::tempdir().unwrap();
    let hub = initialized_hub(&install, &src);
    let proj = project.path().display().to_string();
    assert_eq!(
        json(&hub, &["use", "demo", "-a", "universal", "-p", &proj]).code,
        0,
        "use 夹具应成功"
    );
    (install, project, hub)
}

/// 铺好一个技能的项目场景：hub 内已入库 demo，
/// 项目内已由 universal 与 dsh 各铺一处落点，登记簿留有这条项目记录。
/// 两个 TempDir 守卫由调用方持有整个返回值到用例结束，避免临时目录提前清理
pub struct UsedSkill {
    /// hub 根守卫
    pub install: tempfile::TempDir,
    /// 项目根守卫
    pub project: tempfile::TempDir,
    /// hub 根路径
    pub hub: String,
    /// 项目根路径
    pub proj: String,
    /// 已铺的 agent 名单，与登记簿里的记录一致
    pub agents: &'static [&'static str],
    /// universal 的项目落点（项目根下 .agents/skills/demo）
    pub dest_agents: PathBuf,
    /// dsh 的项目落点（项目根下 .dsh/skills/demo）
    pub dest_dsh: PathBuf,
}

/// 建一个已铺技能的 hub：init → 入库 demo → 项目里 use 一轮（universal 与 dsh）。
/// 需要全局用法、特殊 agent 组合或未铺状态的用例不适用此夹具，自行拼装开场
pub fn hub_with_used_skill() -> UsedSkill {
    let (install, src) = env_with_source();
    let hub = initialized_hub(&install, &src);
    let project = tempfile::tempdir().unwrap();
    let proj = project.path().display().to_string();
    assert_eq!(
        json(
            &hub,
            &[
                "use",
                "demo",
                "--agent",
                "universal,dsh",
                "--project",
                &proj
            ]
        )
        .code,
        0,
        "use 夹具应成功"
    );
    let dest_agents = project.path().join(".agents/skills/demo");
    let dest_dsh = project.path().join(".dsh/skills/demo");
    UsedSkill {
        install,
        project,
        hub,
        proj,
        agents: &["universal", "dsh"],
        dest_agents,
        dest_dsh,
    }
}

/// 把家目录指向临时目录的环境：全局技能目录因此落在可控位置。
/// 必须显式覆盖，否则全局落点会落到真实用户目录上
pub fn home_env(home: &str) -> Vec<(&str, &str)> {
    vec![("USERPROFILE", home), ("HOME", home)]
}

/// `actions` 里是否含一步：expected 里点名的字段全部对上才算
pub fn has_step(actions: &Value, expected: Value) -> bool {
    let expected = expected.as_object().unwrap().clone();
    actions.as_array().unwrap().iter().any(|step| {
        expected
            .iter()
            .all(|(field, value)| step.get(field) == Some(value))
    })
}
