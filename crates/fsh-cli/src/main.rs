//! fsh 命令行：入口与命令分发，业务逻辑全部在 fsh-core，参数定义在
//! cli 模块、输出塑形在 output 模块。输出契约：--json 时 stdout 一行
//! 统一信封 `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 退出码 0 成功 / 1 错误（含 doctor 出 FAIL）。

mod cli;
mod output;

use std::path::Path;

use clap::Parser;
use fsh_core::paths::HubPaths;
use fsh_core::{agents, doctor, gc, hub, project, skill};
use serde_json::{Value, json};

use cli::Cli;
use output::{Failure, cmd_name, emit_err, emit_ok, fail};

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
    let hub_root = match HubPaths::discover(cli.hub.as_deref()) {
        Ok(paths) => paths.root().to_path_buf(),
        // 定位失败发生在日志初始化之前：失败信封是唯一出口，不做静默降级
        Err(e) => {
            emit_err(cli.json, &fail(e));
            std::process::exit(1);
        }
    };
    // 日志目录由日志模块自建：不拿「hub 目录已存在」当前置条件，否则首次 init
    // 这条最该留痕的命令反而没有日志；guard 活到进程退出前，drop 时把缓冲刷盘
    let log_guard = fsh_core::logging::init_logging(&HubPaths::from_root(&hub_root));
    let started = std::time::Instant::now();
    let code = match dispatch(&hub_root, &cli.cmd) {
        Ok((data, code)) => {
            emit_ok(cli.json, &data);
            // 命令完成记在 main 的执行边界：一条覆盖全部子命令，子命令分支里不重复记
            tracing::info!(
                cmd = %cmd_name(&cli.cmd),
                ok = true,
                code,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "命令完成"
            );
            code
        }
        Err(f) => {
            emit_err(cli.json, &f);
            // 失败同记 INFO：非 0 退出是 CLI 的常规结果，不是需要人介入的故障
            tracing::info!(
                cmd = %cmd_name(&cli.cmd),
                ok = false,
                code = 1,
                elapsed_ms = started.elapsed().as_millis() as u64,
                cause = %f.message,
                "命令完成"
            );
            1
        }
    };
    // process::exit 不跑析构，先手动释放 guard 保证日志落盘
    drop(log_guard);
    std::process::exit(code);
}

/// 命令分发：成功返回 (data, 退出码)；非 0 只见于 doctor FAIL——
/// 命令本身执行成功，data 照常携带完整报告
fn dispatch(hub_root: &Path, cmd: &cli::Cmd) -> Result<(Value, i32), Failure> {
    match cmd {
        cli::Cmd::Init { dry_run } => {
            let outcome = hub::init(hub_root, *dry_run).map_err(fail)?;
            Ok((json!(outcome), 0))
        }
        cli::Cmd::Add {
            dir,
            prefix,
            dry_run,
        } => {
            let r = skill::add(hub_root, dir, prefix.as_deref(), *dry_run).map_err(fail)?;
            Ok((json!(r), 0))
        }
        cli::Cmd::List { tag } => {
            let rows = skill::list(hub_root, tag.as_deref()).map_err(fail)?;
            Ok((json!({"skills": rows}), 0))
        }
        cli::Cmd::Show { skill: name } => {
            let d = skill::show(hub_root, name).map_err(fail)?;
            Ok((json!(d), 0))
        }
        cli::Cmd::Release {
            skill: name,
            dry_run,
        } => {
            let r = skill::release(hub_root, name, *dry_run).map_err(fail)?;
            Ok((json!(r), 0))
        }
        cli::Cmd::Archive {
            skill: name,
            dry_run,
        } => {
            let r = skill::archive(hub_root, name, *dry_run).map_err(fail)?;
            Ok((json!(r), 0))
        }
        cli::Cmd::Remove {
            skill: name,
            dry_run,
        } => {
            let r = skill::remove(hub_root, name, *dry_run).map_err(fail)?;
            Ok((json!(r), 0))
        }
        cli::Cmd::Restore {
            skill: name,
            dry_run,
        } => {
            let r = skill::restore(hub_root, name, *dry_run).map_err(fail)?;
            Ok((json!(r), 0))
        }
        cli::Cmd::Agents => {
            let paths = HubPaths::from_root(hub_root);
            let rows = agents::list(&paths).map_err(fail)?;
            Ok((json!({"agents": rows}), 0))
        }
        cli::Cmd::Projects => {
            let rows = project::list(hub_root).map_err(fail)?;
            Ok((json!({"projects": rows}), 0))
        }
        cli::Cmd::Use {
            skill: spec,
            agent,
            global,
            project: project_dirs,
            dry_run,
        } => {
            let (key, version) = project::split_skill_spec(spec).map_err(fail)?;
            let targets = project::Targets::new(*global, project_dirs.clone());
            let r =
                project::use_skill(hub_root, &key, version.as_deref(), agent, targets, *dry_run)
                    .map_err(fail)?;
            Ok((json!(r), 0))
        }
        cli::Cmd::Unuse {
            skill: key,
            agent,
            global,
            project: project_dirs,
            force,
            dry_run,
        } => {
            let targets = project::Targets::new(*global, project_dirs.clone());
            let r = project::unuse(hub_root, key, targets, agent.as_deref(), *force, *dry_run)
                .map_err(fail)?;
            Ok((json!(r), 0))
        }
        cli::Cmd::Diff {
            skill: name,
            v1,
            v2,
        } => {
            let files = skill::diff(hub_root, name, v1, v2).map_err(fail)?;
            Ok((json!({"files": files}), 0))
        }
        cli::Cmd::Gc { dry_run } => {
            let r = gc::gc(hub_root, *dry_run).map_err(fail)?;
            Ok((json!({"actions": r.items, "freed_bytes": r.freed_bytes}), 0))
        }
        cli::Cmd::Doctor => {
            let r = doctor::doctor(hub_root).map_err(fail)?;
            let has_fail = r
                .checks
                .iter()
                .any(|c| c.status == doctor::CheckStatus::Fail);
            let code = if has_fail { 1 } else { 0 };
            Ok((json!({"checks": r.checks, "fail": has_fail}), code))
        }
    }
}
