//! fsh 命令行：只做参数解析与输出格式化，业务逻辑全部在 fsh-core。
//! 输出契约：--json 时 stdout 一行统一信封
//! `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 退出码 0 成功 / 1 错误（含 doctor 出 FAIL）/ 2 status 漂移。

use std::fmt::Display;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use fsh_core::ErrorCode;
use fsh_core::paths::HubPaths;
use fsh_core::skill::BumpPart;
use fsh_core::{backup, deploy, doctor, gc, hub, skill};
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
    /// 入库一个本地 skill 目录（复制 + 首版 tag + 物化，一步到位）
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
    /// 发版：把工作副本当前全部改动（含脏）打成新版本
    Version {
        skill: String,
        /// 代算版本号段位（缺省按 frontmatter 里写的号发版）
        #[arg(long)]
        bump: Option<BumpCli>,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 删除 skill（默认归档保版本；--purge 连全部版本 tag 一起删）
    Remove {
        skill: String,
        /// 彻底删除：连版本 tag 一起删（被项目引用时拒绝）
        #[arg(long)]
        purge: bool,
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
    /// 比较同一 skill 的两个版本（unified diff）
    Diff {
        skill: String,
        v1: String,
        v2: String,
    },
    /// 预演项目变更（等价 apply --dry-run，输出 Change 清单）
    Plan {
        /// 项目目录（缺省当前目录）
        #[arg(default_value = ".")]
        project: PathBuf,
    },
    /// 把 skills.toml 声明铺成链接（写 lock/state，顺手清旧物化）
    Apply {
        /// 项目目录（缺省当前目录）
        #[arg(default_value = ".")]
        project: PathBuf,
        /// 只列将执行的变更，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 检查项目漂移（有漂移时退出码 2）
    Status {
        /// 项目目录（缺省当前目录）
        #[arg(default_value = ".")]
        project: PathBuf,
    },
    /// 清理无引用的物化目录（内容与 tag 不符的绝不删）
    Gc {
        /// 只列将清理的项与可释放字节，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 环境体检（有 FAIL 项时退出码 1）
    Doctor,
    /// 把整个 hub 打包为 zip 放到目标目录
    Backup {
        /// 备份 zip 输出目录
        dest: PathBuf,
        /// 只报将写出的备份文件，不落盘
        #[arg(long)]
        dry_run: bool,
    },
}

/// version --bump 的段位取值
#[derive(Clone, Copy, ValueEnum)]
enum BumpCli {
    /// 补丁位 x.y.Z
    Patch,
    /// 次版本位 x.Y.0
    Minor,
    /// 主版本位 X.0.0
    Major,
}

impl From<BumpCli> for BumpPart {
    fn from(v: BumpCli) -> Self {
        match v {
            BumpCli::Patch => BumpPart::Patch,
            BumpCli::Minor => BumpPart::Minor,
            BumpCli::Major => BumpPart::Major,
        }
    }
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

/// 命令分发：成功返回 (data, 退出码)；退出码非 0 的场景仅 status 漂移（2）
/// 与 doctor FAIL（1），二者命令本身执行成功，data 照常携带完整报告
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
        Cmd::Version {
            skill: name,
            bump,
            dry_run,
        } => {
            if *dry_run {
                let actions =
                    skill::bump_dry(hub_root, name, bump.map(Into::into)).map_err(fail)?;
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            let next = skill::version_bump(hub_root, name, bump.map(Into::into)).map_err(fail)?;
            Ok((json!({"key": name, "version": next}), 0))
        }
        Cmd::Remove {
            skill: name,
            purge,
            dry_run,
        } => {
            if *dry_run {
                let actions = skill::remove_dry(hub_root, name, *purge).map_err(fail)?;
                return Ok((
                    json!({"dry_run": true, "purge": purge, "actions": actions}),
                    0,
                ));
            }
            let r = skill::remove(hub_root, name, *purge).map_err(fail)?;
            Ok((json!({"key": r.key, "purged": r.purged}), 0))
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
        Cmd::Diff {
            skill: name,
            v1,
            v2,
        } => {
            let files = skill::diff(hub_root, name, v1, v2).map_err(fail)?;
            Ok((json!({"files": files}), 0))
        }
        Cmd::Plan { project } => {
            let changes = deploy::plan(hub_root, project).map_err(fail)?;
            Ok((json!({"changes": changes}), 0))
        }
        Cmd::Apply { project, dry_run } => {
            // apply 的预演就是 plan：同一套对比逻辑，输出 Change 清单
            if *dry_run {
                let changes = deploy::plan(hub_root, project).map_err(fail)?;
                return Ok((json!({"dry_run": true, "changes": changes}), 0));
            }
            let changes = deploy::apply(hub_root, project).map_err(fail)?;
            Ok((json!({"changes": changes}), 0))
        }
        Cmd::Status { project } => {
            let drifts = deploy::status(hub_root, project).map_err(fail)?;
            let clean = drifts.is_empty();
            // 漂移是命令的正常发现而非执行失败：ok 信封照发，退出码 2 供脚本分支
            let code = if clean { 0 } else { 2 };
            Ok((json!({"clean": clean, "drift": drifts}), code))
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
        Cmd::Backup { dest, dry_run } => {
            if *dry_run {
                // 动作清单由 core 生成：zip 目标名与真实执行同源同格式
                let actions = backup::backup_dry(hub_root, dest);
                return Ok((json!({"dry_run": true, "actions": actions}), 0));
            }
            let p = backup::backup(hub_root, dest).map_err(fail)?;
            Ok((json!({"path": p.display().to_string()}), 0))
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
