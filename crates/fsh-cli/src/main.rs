//! fsh 命令行：入口与命令分发，业务逻辑全部在 fsh-core，参数定义在
//! cli 模块、输出塑形在 output 模块。输出契约：--json 时 stdout 一行
//! 统一信封 `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 退出码 0 成功 / 1 错误（含 doctor 出 FAIL）。

mod cli;
mod output;

use std::path::Path;

use clap::{CommandFactory, FromArgMatches};
use fsh_core::paths::HubPaths;
use fsh_core::search::ContextWindow;
use fsh_core::select::Selector;
use fsh_core::{doctor, hub, project, search, select, skill};

use cli::Cli;
use output::{
    CliError, Failure, Outcome, SearchView, cmd_name, doctor_outcome, emit_err, emit_ok, fail,
    init_outcome, list_outcome, move_outcome, search_outcome, selection_outcome, unuse_outcome,
};

fn main() {
    // 解析失败时还没有 Cli 结构，--json 只能按原始参数粗判
    let want_json = std::env::args().any(|arg| arg == "--json");
    let cli = match cli::localize(Cli::command()).try_get_matches() {
        Ok(matches) => match Cli::from_arg_matches(&matches) {
            Ok(c) => c,
            Err(e) => {
                e.print().ok();
                std::process::exit(1);
            }
        },
        Err(e) => {
            // --help / --version 走 clap 原生输出（退出码 0）；
            // 其余用法错误：带 --json 出信封，人读模式交给 clap 渲染到 stderr
            if e.use_stderr() {
                if want_json {
                    emit_err(true, &fail(CliError::Usage(e.to_string())));
                } else {
                    e.print().ok();
                }
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
            emit_ok(&oc);
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
            Ok(init_outcome(&r, hub_root, json))
        }
        cli::Cmd::Add {
            dir,
            prefix,
            skill: names,
            all,
            dry_run,
        } => {
            let selector = add_selector(prefix.clone(), names.clone(), *all);
            let r = skill::add(hub_root, dir, &selector, *dry_run).map_err(fail)?;
            Ok(selection_outcome(&r, json))
        }
        cli::Cmd::List {
            filter,
            prefix,
            archived,
        } => {
            let r = skill::list(hub_root, filter.as_deref(), *archived, prefix.as_deref())
                .map_err(fail)?;
            Ok(list_outcome(&r, *archived, json))
        }
        cli::Cmd::Archive {
            prefix,
            skill: names,
            all,
            dry_run,
        } => {
            let selector = selector_from_args(prefix.clone(), names.clone(), *all)?;
            let r = skill::archive(hub_root, &selector, *dry_run).map_err(fail)?;
            Ok(selection_outcome(&r, json))
        }
        cli::Cmd::Remove {
            prefix,
            skill: names,
            all,
            dry_run,
        } => {
            let selector = selector_from_args(prefix.clone(), names.clone(), *all)?;
            let r = skill::remove(hub_root, &selector, *dry_run).map_err(fail)?;
            Ok(selection_outcome(&r, json))
        }
        cli::Cmd::Restore {
            prefix,
            skill: names,
            all,
            dry_run,
        } => {
            let selector = selector_from_args(prefix.clone(), names.clone(), *all)?;
            let r = skill::restore(hub_root, &selector, *dry_run).map_err(fail)?;
            Ok(selection_outcome(&r, json))
        }
        cli::Cmd::Move {
            prefix,
            skill: name,
            to,
            dry_run,
        } => {
            let r =
                skill::move_skill(hub_root, prefix.clone(), name, to, *dry_run).map_err(fail)?;
            Ok(move_outcome(&r, json))
        }
        cli::Cmd::Use {
            prefix,
            skill: names,
            all,
            agent,
            global,
            project: project_dirs,
            dry_run,
        } => {
            let selector = selector_from_args(prefix.clone(), names.clone(), *all)?;
            let targets = project::Targets::new(*global, project_dirs.clone());
            let r =
                project::use_skill(hub_root, &selector, agent, targets, *dry_run).map_err(fail)?;
            Ok(selection_outcome(&r, json))
        }
        cli::Cmd::Unuse {
            prefix,
            skill: names,
            all,
            agent,
            global,
            project: project_dirs,
            force,
            dry_run,
        } => {
            let selector = selector_from_args(prefix.clone(), names.clone(), *all)?;
            let targets = project::Targets::new(*global, project_dirs.clone());
            let r = project::unuse(hub_root, &selector, targets, agent, *force, *dry_run)
                .map_err(fail)?;
            Ok(unuse_outcome(&r, json))
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
                return Err(fail(CliError::Usage(
                    "上下文 flag（-A/-B/-C）仅文本模式可用，与 --json 互斥".to_string(),
                )));
            }
            // 上下文并进检索请求：行组由 core 在截断后展开，文本模式直接取用
            let window = ContextWindow::from_flags(*after, *before, *context);
            let opts = search::SearchOptions {
                path: path.clone(),
                case_insensitive: *case_insensitive,
                literal: *literal,
                whole_word: *whole_word,
                globs: glob.clone(),
                limit: *limit,
                context: window,
            };
            let res = search::search(hub_root, pattern, &opts).map_err(fail)?;
            let view = SearchView::of(*files_with_matches, *count, window);
            Ok(search_outcome(&res, view, json))
        }
        cli::Cmd::Doctor => {
            let r = doctor::doctor(hub_root).map_err(fail)?;
            Ok(doctor_outcome(&r, json))
        }
    }
}

/// 入库的选择器：`--all` 与 `--skill` 由 clap 判互斥；两者都没给时是一份空点名，
/// 由入库按「源里恰好一个才成立」收下。前缀可省（=根），范围由源目录自己划定——
/// 因此不走 `selector_from_args` 那条「--all 必须有前缀」的用法判定
fn add_selector(prefix: Option<String>, names: Vec<String>, all: bool) -> Selector {
    if all {
        return Selector::all(prefix);
    }
    Selector::named(prefix, names)
}

/// 选择器的命令行形状：`--all` 要的是「这套里的全部」，没有前缀就没有套——
/// 因而不存在整库批量操作的入口，按用法错误拒绝
fn selector_from_args(
    prefix: Option<String>,
    names: Vec<String>,
    all: bool,
) -> Result<Selector, Failure> {
    if !all {
        return Ok(Selector::named(prefix, names));
    }
    if select::normalize(prefix.as_deref()).is_none() {
        return Err(fail(CliError::Usage(
            "--all 需要一个非空白的前缀：不存在整库批量操作".to_string(),
        )));
    }
    Ok(Selector::all(prefix))
}
