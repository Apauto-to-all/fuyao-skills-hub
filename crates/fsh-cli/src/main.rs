//! fsh 命令行：入口与命令分发，业务逻辑全部在 fsh-core，参数定义在
//! cli 模块、输出塑形在 output 模块。输出契约：--json 时 stdout 一行
//! 统一信封 `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 退出码 0 成功 / 1 错误（含 doctor 出 FAIL）。

mod cli;
mod output;

use std::path::Path;

use clap::Parser;
use fsh_core::paths::HubPaths;
use fsh_core::{doctor, hub, project, search, skill};
use serde_json::json;

use cli::Cli;
use output::{
    Failure, Outcome, cmd_name, doctor_outcome, emit_err, emit_ok, fail, list_outcome, plain,
    query, search_text_outcome,
};

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
    let code = match dispatch(&hub_root, &cli.cmd, cli.json) {
        Ok(oc) => {
            emit_ok(cli.json, &oc);
            // 命令完成记在 main 的执行边界：一条覆盖全部子命令，子命令分支里不重复记。
            // ok 取退出码：信封照常是成功，结果却可能有 doctor FAIL
            tracing::info!(
                cmd = %cmd_name(&cli.cmd),
                ok = oc.code == 0,
                code = oc.code,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "命令完成"
            );
            oc.code
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

/// 命令分发：执行命令并给出输出结果；非 0 退出码见于 doctor 查出 FAIL。
/// 命令本身执行完成，data 照常携带完整报告
fn dispatch(hub_root: &Path, cmd: &cli::Cmd, json: bool) -> Result<Outcome, Failure> {
    match cmd {
        cli::Cmd::Init { dry_run } => {
            let r = hub::init(hub_root, *dry_run).map_err(fail)?;
            Ok(plain(&r, 0))
        }
        cli::Cmd::Add {
            dir,
            prefix,
            dry_run,
        } => {
            let r = skill::add(hub_root, dir, prefix.as_deref(), *dry_run).map_err(fail)?;
            Ok(plain(&r, 0))
        }
        cli::Cmd::List { filter, archived } => {
            let r = skill::list(hub_root, filter.as_deref(), *archived).map_err(fail)?;
            Ok(list_outcome(&r, *archived))
        }
        cli::Cmd::Archive {
            skill: name,
            dry_run,
        } => {
            let r = skill::archive(hub_root, name, *dry_run).map_err(fail)?;
            Ok(plain(&r, 0))
        }
        cli::Cmd::Remove {
            skill: name,
            dry_run,
        } => {
            let r = skill::remove(hub_root, name, *dry_run).map_err(fail)?;
            Ok(plain(&r, 0))
        }
        cli::Cmd::Restore {
            skill: name,
            dry_run,
        } => {
            let r = skill::restore(hub_root, name, *dry_run).map_err(fail)?;
            Ok(plain(&r, 0))
        }
        cli::Cmd::Move { from, to, dry_run } => {
            let r = skill::move_skill(hub_root, from, to, *dry_run).map_err(fail)?;
            Ok(plain(&r, 0))
        }
        cli::Cmd::Use {
            skill: spec,
            agent,
            global,
            project: project_dirs,
            dry_run,
        } => {
            // 键原样下传：形状判定在 core 的入口统一做，CLI 不另立一套规则
            let targets = project::Targets::new(*global, project_dirs.clone());
            let r = project::use_skill(hub_root, spec, agent, targets, *dry_run).map_err(fail)?;
            Ok(query(&r, 0))
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
            let r =
                project::unuse(hub_root, key, targets, agent, *force, *dry_run).map_err(fail)?;
            Ok(query(&r, 0))
        }
        cli::Cmd::Search {
            pattern,
            path,
            case_insensitive,
            literal,
            whole_word,
            glob,
            files_with_matches,
            count,
            after,
            before,
            context,
            limit,
        } => {
            // -A/-B/-C 只属于文本模式：--json 是全局 flag，跨层冲突注册
            // 不生效，在执行边界统一挡下，错误口径与用法错误一致
            if json && (after.is_some() || before.is_some() || context.is_some()) {
                return Err(Failure {
                    code: "usage",
                    message: "上下文 flag（-A/-B/-C）仅文本模式可用，与 --json 互斥".to_string(),
                });
            }
            let opts = search::SearchOptions {
                path: path.clone(),
                case_insensitive: *case_insensitive,
                literal: *literal,
                whole_word: *whole_word,
                globs: glob.clone(),
                limit: *limit,
            };
            let res = search::search(hub_root, pattern, &opts).map_err(fail)?;
            // JSON 模式出检索信封（hits/total/truncated，命中含键投影与
            // spans）；文本模式才做 -l/-c/上下文塑形
            if json {
                return Ok(query(&res, 0));
            }
            let text = output::render_search(
                hub_root,
                &res,
                *files_with_matches,
                *count,
                (*after, *before, *context),
            );
            Ok(search_text_outcome(text))
        }
        cli::Cmd::Doctor => {
            let r = doctor::doctor(hub_root).map_err(fail)?;
            Ok(doctor_outcome(&r))
        }
    }
}
