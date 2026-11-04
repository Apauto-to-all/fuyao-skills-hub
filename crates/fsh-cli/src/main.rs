//! fsh 命令行：只做参数解析与输出格式化，业务逻辑全部在 fsh-core。
//! 输出契约：--json 时 stdout 一行统一信封
//! `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 退出码 0 成功 / 1 错误（含 doctor 出 FAIL）。

use std::fmt::Display;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use fsh_core::ErrorCode;
use fsh_core::paths::HubPaths;
use fsh_core::{agents, doctor, gc, hub, project, skill};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "fsh", version, about = "fuyao-skills-hub 技能库命令行")]
struct Cli {
    /// hub 根目录（缺省用可执行文件同级 data\）
    #[arg(long, global = true)]
    hub: Option<PathBuf>,
    /// 以 JSON 信封输出（stdout 一行）
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 初始化 hub（幂等：已初始化只报告，一字节不动）
    Init {
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 入库一个本地 skill 目录（复制 + 首版 tag，一步到位）
    Add {
        /// skill 源目录（须含带 frontmatter 的 SKILL.md）
        dir: PathBuf,
        /// 键的组织前缀（如 anthropics/skills），缺省入根下
        #[arg(long)]
        prefix: Option<String>,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 列出全部 skill（含归档态）
    List {
        /// 按 frontmatter tags 过滤
        #[arg(long)]
        tag: Option<String>,
    },
    /// 查看单个 skill 全量详情（frontmatter / 版本时间线 / 被引用项目）
    Show { skill: String },
    /// 发版：把工作副本当前全部改动（含脏）按 frontmatter 里的号打成新版本
    Release {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 归档 skill（删编辑面与导出目录，版本 tag 与历史全留，可 restore）
    Archive {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 彻底删除 skill（连全部版本 tag 一起删；被项目引用时拒绝）
    Remove {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 从最新版本恢复归档的 skill（复活编辑面，不打新 tag）
    Restore {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 列出认得的 agent（内置 + 用户覆盖）：名字、显示名、目录、配置来源
    Agents,
    /// 首次把 skill 铺给某项目的几个 agent（未归档 + 无登记 + 落点全空闲）
    Use {
        /// 技能说明：`<skill>` 跟随最新，`<skill>@<版本>` 用固定版本
        skill: String,
        /// 目标 agent，逗号分隔
        #[arg(long, value_delimiter = ',')]
        agent: Vec<String>,
        /// 项目根目录（须已存在），缺省当前目录
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 整体撤回某项目的某技能（该项目全部 agent 落点）
    Unuse {
        skill: String,
        /// 项目根目录（须已存在），缺省当前目录
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 覆盖「copy 内容被修改」的安全闸
        #[arg(long)]
        force: bool,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 比较同一 skill 的两个版本（unified diff）
    Diff {
        skill: String,
        v1: String,
        v2: String,
    },
    /// 清理无引用的导出目录（内容与 tag 不符的绝不删）
    Gc {
        /// 只列将清理的项与可释放字节，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 环境体检（有 FAIL 项时退出码 1）
    Doctor,
}

/// 失败信封载荷：code 取 core 错误的稳定码（ErrorCode 契约，显式声明、
/// 包装变体透传底层真因），message 取 Display 文本
struct Failure {
    code: &'static str,
    message: String,
}

/// 任意 core 错误统一收敛为失败信封
fn fail(e: impl Display + ErrorCode) -> Failure {
    Failure {
        code: e.error_code(),
        message: e.to_string(),
    }
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            // --help / --version 走 clap 原生输出（退出码 0）；
            // 其余用法错误统一走失败信封（退出码 1）
            if e.use_stderr() {
                println!(
                    "{}",
                    json!({"ok": false, "error": {"code": "usage", "message": e.to_string()}})
                );
                std::process::exit(1);
            }
            e.print().ok();
            std::process::exit(0);
        }
    };
    let hub_root = HubPaths::discover(cli.hub.as_deref()).root().to_path_buf();
    match dispatch(&hub_root, &cli.cmd) {
        Ok((data, code)) => {
            emit_ok(cli.json, &data);
            std::process::exit(code);
        }
        Err(f) => {
            emit_err(cli.json, &f);
            std::process::exit(1);
        }
    }
}

/// 命令分发：成功返回 (data, 退出码)；非 0 只见于 doctor FAIL——
/// 命令本身执行成功，data 照常携带完整报告
fn dispatch(hub_root: &Path, cmd: &Cmd) -> Result<(Value, i32), Failure> {
    match cmd {
        Cmd::Init { dry_run } => {
            // --hub 指向 hub 根；安装目录换算归布局模块单一入口
            let paths = HubPaths::from_root(hub_root);
            let install = paths.install();
            let outcome = hub::init(install, *dry_run).map_err(fail)?;
            Ok((json!(outcome), 0))
        }
        Cmd::Add {
            dir,
            prefix,
            dry_run,
        } => {
            if *dry_run {
                let actions = skill::add_dry(hub_root, dir, prefix.as_deref()).map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            let r = skill::add(hub_root, dir, prefix.as_deref()).map_err(fail)?;
            Ok((json!({"key": r.key, "version": r.version}), 0))
        }
        Cmd::List { tag } => {
            let rows = skill::list(hub_root, tag.as_deref()).map_err(fail)?;
            Ok((json!({"skills": rows}), 0))
        }
        Cmd::Show { skill: name } => {
            let d = skill::show(hub_root, name).map_err(fail)?;
            Ok((json!(d), 0))
        }
        Cmd::Release {
            skill: name,
            dry_run,
        } => {
            if *dry_run {
                let actions = skill::release_dry(hub_root, name).map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            let next = skill::release(hub_root, name).map_err(fail)?;
            Ok((json!({"key": name, "version": next}), 0))
        }
        Cmd::Archive {
            skill: name,
            dry_run,
        } => {
            if *dry_run {
                let actions = skill::archive_dry(hub_root, name).map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            skill::archive(hub_root, name).map_err(fail)?;
            Ok((json!({"key": name, "archived": true}), 0))
        }
        Cmd::Remove {
            skill: name,
            dry_run,
        } => {
            if *dry_run {
                let actions = skill::remove_dry(hub_root, name).map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            skill::remove(hub_root, name).map_err(fail)?;
            Ok((json!({"key": name, "removed": true}), 0))
        }
        Cmd::Restore {
            skill: name,
            dry_run,
        } => {
            if *dry_run {
                let actions = skill::restore_dry(hub_root, name).map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            skill::restore(hub_root, name).map_err(fail)?;
            Ok((json!({"key": name, "restored": true}), 0))
        }
        Cmd::Agents => {
            let paths = HubPaths::from_root(hub_root);
            let rows = agents::list(&paths).map_err(fail)?;
            Ok((json!({"agents": rows}), 0))
        }
        Cmd::Use {
            skill: spec,
            agent,
            project: project_dir,
            dry_run,
        } => {
            let (key, version) = project::split_skill_spec(spec).map_err(fail)?;
            if *dry_run {
                let actions =
                    project::use_dry(hub_root, &key, version.as_deref(), agent, project_dir)
                        .map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            let r = project::use_skill(
                hub_root,
                &key,
                version.as_deref(),
                agent,
                project_dir,
                false,
            )
            .map_err(fail)?;
            Ok((json!(r), 0))
        }
        Cmd::Unuse {
            skill: key,
            project: project_dir,
            force,
            dry_run,
        } => {
            if *dry_run {
                let actions =
                    project::unuse_dry(hub_root, key, project_dir, *force).map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            let r = project::unuse(hub_root, key, project_dir, *force, false).map_err(fail)?;
            Ok((json!(r), 0))
        }
        Cmd::Diff {
            skill: name,
            v1,
            v2,
        } => {
            let files = skill::diff(hub_root, name, v1, v2).map_err(fail)?;
            Ok((json!({"files": files}), 0))
        }
        Cmd::Gc { dry_run } => {
            let r = gc::gc(hub_root, *dry_run).map_err(fail)?;
            Ok((json!({"actions": r.items, "freed_bytes": r.freed_bytes}), 0))
        }
        Cmd::Doctor => {
            // doctor 吃安装目录；换算归布局模块单一入口
            let paths = HubPaths::from_root(hub_root);
            let install = paths.install();
            let r = doctor::doctor(install).map_err(fail)?;
            let has_fail = r
                .checks
                .iter()
                .any(|c| c.status == doctor::CheckStatus::Fail);
            let code = if has_fail { 1 } else { 0 };
            Ok((json!({"checks": r.checks, "fail": has_fail}), code))
        }
    }
}

/// 成功输出：--json 一行信封；文本模式动作清单逐行、diff 逐文件输出
/// unified patch 原文（人读优先）、其余 pretty JSON
fn emit_ok(json: bool, data: &Value) {
    if json {
        println!("{}", json!({"ok": true, "data": data}));
        return;
    }
    if let Some(actions) = data.get("actions").and_then(Value::as_array) {
        for a in actions {
            println!("- {a}");
        }
        return;
    }
    // diff 的文件数组：patch 字段本身就是 unified diff 原文，直接打印
    if let Some(files) = data.get("files").and_then(Value::as_array) {
        for f in files {
            println!("diff {}", f["path"].as_str().unwrap_or(""));
            if let Some(p) = f["patch"].as_str() {
                print!("{p}");
            }
        }
        return;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".to_string())
    );
}

/// 失败输出：--json 一行信封；文本模式进 stderr（不污染 stdout 数据流）
fn emit_err(json: bool, f: &Failure) {
    if json {
        println!(
            "{}",
            json!({"ok": false, "error": {"code": f.code, "message": f.message}})
        );
        return;
    }
    eprintln!("错误 [{}]: {}", f.code, f.message);
}
