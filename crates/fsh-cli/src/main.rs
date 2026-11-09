//! fsh 命令行：入口与命令分发，业务逻辑全部在 fsh-core，参数定义在
//! cli 模块、输出塑形在 output 模块。输出契约：--json 时 stdout 一行
//! 统一信封 `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 退出码 0 成功 / 1 错误（含 doctor 出 FAIL）。

mod cli;
mod output;

use std::path::Path;

use clap::Parser;
use fsh_core::paths::HubPaths;
use fsh_core::{agents, doctor, gc, hub, project, search, skill};
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
    let code = match dispatch(&hub_root, &cli.cmd, cli.json) {
        Ok((data, code)) => {
            emit_ok(cli.json, &data);
            // 命令完成记在 main 的执行边界：一条覆盖全部子命令，子命令分支里不重复记。
            // ok 取退出码：信封照常是成功，结果却可能有 doctor FAIL 或批量升级中途失败
            tracing::info!(
                cmd = %cmd_name(&cli.cmd),
                ok = code == 0,
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

/// 命令分发：成功返回 (data, 退出码)；非 0 见于两种情况——doctor 查出 FAIL、
/// 批量升级中途失败。两者都是命令本身执行完成，data 照常携带完整报告
fn dispatch(hub_root: &Path, cmd: &cli::Cmd, json: bool) -> Result<(Value, i32), Failure> {
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
            let r = skill::release(
                hub_root,
                name,
                skill::ReleaseSource::Frontmatter,
                false,
                *dry_run,
            )
            .map_err(fail)?;
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
        cli::Cmd::Upgrade {
            skill,
            all,
            global,
            project: project_dirs,
            force,
            dry_run,
        } => {
            let targets = project::Targets::new(*global, project_dirs.clone());
            // 批量形态：<skill> 与 --all 二选一由参数解析挡下
            if *all {
                // 两个范围 flag 都不给才是全 hub，给一个就只把范围收窄
                let scope = (*global || !project_dirs.is_empty()).then_some(targets);
                let r = project::upgrade_all(hub_root, scope, *force, *dry_run).map_err(fail)?;
                // 实跑中途失败即停：报告照出（已完成与未开始都在），退出码 1
                let code = if r.stopped.is_some() { 1 } else { 0 };
                return Ok((json!(r), code));
            }
            // 单条形态：没给 --all 就一定有 <skill>，缺了也只会落到「技能说明非法」这一条错误
            let (key, version) =
                project::split_skill_spec(skill.as_deref().unwrap_or_default()).map_err(fail)?;
            let r = project::upgrade(
                hub_root,
                &key,
                version.as_deref(),
                targets,
                *force,
                *dry_run,
            )
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
                context: None,
            };
            let res = search::search(hub_root, pattern, &opts).map_err(fail)?;
            // JSON 模式出检索信封（hits/total/truncated，命中含键投影与
            // spans）；文本模式才做 -l/-c/上下文塑形
            if json {
                return Ok((json!(res), 0));
            }
            let text = output::render_search(
                hub_root,
                &res,
                *files_with_matches,
                *count,
                (*after, *before, *context),
            );
            Ok((json!({ "search_text": text }), 0))
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
