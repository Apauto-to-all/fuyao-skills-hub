//! 内置技能对齐：把编译期内嵌的技能表整份物化进 `store\builtin\`，
//! 让 `fsh init` 之后内置技能就是库里一个普通技能。
//! 对齐以程序内嵌内容为准：键缺失整份落地、内容不同整份覆盖、
//! 内容相同一个字节不动、内嵌里已无的键整份删掉。
//! 对齐计划一处产出：预演把它翻成动作清单，实跑按同一份计划落盘；
//! 锁序、封存手改与机器提交都走写入口的共用骨架。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::HubError;
use crate::bundled::BuiltinSkill;
use crate::commit::{Face, Verb, message};
use crate::naming::{BUILTIN_PREFIX, builtin_key};
use crate::paths::HubPaths;
use crate::plan::{ActionKind, PlanStep};
use crate::skill::skill_occupancy;
use crate::write::{self, Applied};

/// init 的收尾一步：把 `store\builtin\` 与内嵌技能表整份对齐。
/// 键由 store 内相对路径得出（`builtin/<目录名>`），不另造键、不做名字映射；
/// dry_run = true 时只算动作清单，不落盘、不取写锁
pub(crate) fn sync_builtin(
    paths: &HubPaths,
    skills: &[BuiltinSkill],
    dry_run: bool,
) -> Result<Vec<PlanStep>, HubError> {
    if dry_run {
        return Ok(plan_builtin_with(paths, skills)?
            .iter()
            .flat_map(|plan| plan.actions(paths))
            .collect());
    }
    let root = paths.root();
    let total = skills.len();
    write::run_assembly(
        root,
        |paths| plan_builtin_with(paths, skills),
        |ctx, plans| {
            let (actions, commits) = land_builtin(ctx.paths, plans)?;
            let milestones = (actions.len(), total);
            Ok(Applied {
                outcome: actions,
                commits,
                milestones,
            })
        },
        |(count, total), elapsed_ms| {
            if count == 0 {
                // 内容一致是幂等命中：重跑 init 的正常结果，DEBUG 留痕不刷屏
                tracing::debug!(count = total, "内置技能内容一致，对齐跳过");
            } else {
                tracing::info!(
                    path = %root.display(),
                    count = count,
                    elapsed_ms = elapsed_ms,
                    "内置技能对齐完成"
                );
            }
        },
    )
}

/// 按计划落盘：逐技能对齐，交回动作清单与机器提交消息。
/// 零动作表示内容一致，不产生提交
fn land_builtin(
    paths: &HubPaths,
    plans: Vec<BuiltinPlan<'_>>,
) -> Result<(Vec<PlanStep>, Vec<String>), HubError> {
    let mut actions = Vec::new();
    for plan in &plans {
        actions.extend(apply_plan(paths, plan)?);
    }
    let commits = if actions.is_empty() {
        Vec::new()
    } else {
        // 对齐有落盘：一次机器提交收编全部内置技能变更
        vec![message(Verb::Init, Face::Builtin, "builtin")]
    };
    Ok((actions, commits))
}

/// 一个内置技能的对齐计划：动作与执行所需数据同体
enum BuiltinPlan<'a> {
    /// 键缺失：整份落地
    Create {
        skill: &'a BuiltinSkill,
        key: String,
    },
    /// 键在场、内容不同：整份覆盖
    Overwrite {
        skill: &'a BuiltinSkill,
        key: String,
    },
    /// 键在场、内容相同：一个字节不动
    Keep,
    /// 内嵌里已无此技能：删工作副本
    Remove { key: String },
}

impl BuiltinPlan<'_> {
    /// 该计划的步骤清单：预演直接输出，实跑按同一份清单执行
    fn actions(&self, paths: &HubPaths) -> Vec<PlanStep> {
        let commit = || PlanStep::commit(message(Verb::Init, Face::Builtin, "builtin"));
        match self {
            BuiltinPlan::Create { key, .. } | BuiltinPlan::Overwrite { key, .. } => vec![
                PlanStep::of(ActionKind::Write, crate::naming::store_subpath(key)),
                commit(),
            ],
            BuiltinPlan::Keep => Vec::new(),
            BuiltinPlan::Remove { key } => {
                if paths.skill_workdir(key).exists() {
                    vec![
                        PlanStep::of(ActionKind::Remove, crate::naming::store_subpath(key)),
                        commit(),
                    ]
                } else {
                    Vec::new()
                }
            }
        }
    }
}

/// 逐技能与孤儿目录一起排计划：内嵌表顺序即执行顺序（目录名升序），孤儿键升序
fn plan_builtin_with<'a>(
    paths: &HubPaths,
    skills: &'a [BuiltinSkill],
) -> Result<Vec<BuiltinPlan<'a>>, HubError> {
    let mut plans: Vec<BuiltinPlan<'a>> = skills
        .iter()
        .map(|skill| plan_skill(paths, skill))
        .collect();
    plans.extend(builtin_orphans(paths, skills)?);
    Ok(plans)
}

/// 单个内置技能的计划：工作副本缺失落地、内容不同覆盖、内容相同不动。
/// 归档副本不参与判定：物化只写 store 一侧
fn plan_skill<'a>(paths: &HubPaths, skill: &'a BuiltinSkill) -> BuiltinPlan<'a> {
    let key = builtin_key(&skill.name);
    let occupancy = skill_occupancy(paths, &key);
    match occupancy.workdir() {
        // 工作副本缺失：整份落地
        None => BuiltinPlan::Create { skill, key },
        Some(_) if workdir_matches(paths, &key, skill) => BuiltinPlan::Keep,
        // 工作副本在场但内容不同：整份覆盖
        Some(_) => BuiltinPlan::Overwrite { skill, key },
    }
}

/// 工作副本是否与内嵌内容逐字节一致（文件集合相同、每个文件字节相同）。
/// 本函数是这一比对的唯一口径
pub(crate) fn workdir_matches(paths: &HubPaths, key: &str, skill: &BuiltinSkill) -> bool {
    let dir = paths.skill_workdir(key);
    let mut on_disk: Vec<String> = Vec::new();
    if collect_disk_files(&dir, "", &mut on_disk).is_err() {
        return false;
    }
    if on_disk.len() != skill.files.len() {
        return false;
    }
    skill.files.iter().all(|file| {
        on_disk.iter().any(|path| path == &file.path)
            && fs::read(dir.join(&file.path)).is_ok_and(|bytes| bytes == file.contents)
    })
}

/// 递归收集工作副本里的文件相对路径（`/` 分隔）
fn collect_disk_files(dir: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = format!("{prefix}{name}");
        if entry.path().is_dir() {
            collect_disk_files(&entry.path(), &format!("{rel}/"), out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

/// 内嵌里已不存在、而 store 里仍有目录的键：整份删掉，不留孤儿。
/// 目录按 `store\builtin\` 的直接子目录收口
fn builtin_orphans<'a>(
    paths: &HubPaths,
    skills: &[BuiltinSkill],
) -> Result<Vec<BuiltinPlan<'a>>, HubError> {
    let embedded: BTreeSet<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
    let orphans = orphan_names(paths, &embedded)?
        .into_iter()
        .map(|name| BuiltinPlan::Remove {
            key: builtin_key(&name),
        })
        .collect();
    Ok(orphans)
}

/// `store\builtin\` 下不在内嵌表里的目录名（升序）：内嵌里已无的技能目录。
/// 本函数是这一收口的唯一扫描
pub(crate) fn orphan_names(
    paths: &HubPaths,
    embedded: &BTreeSet<&str>,
) -> Result<Vec<String>, HubError> {
    let mut names = Vec::new();
    match fs::read_dir(paths.store().join(BUILTIN_PREFIX)) {
        Ok(entries) => {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if !embedded.contains(name.as_str()) {
                    names.push(name);
                }
            }
        }
        // 目录不存在 = 一个孤儿目录都没有
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    names.sort();
    Ok(names)
}

/// 实跑一个计划：只落盘，提交由 land_builtin 在全部计划执行完统一收口
fn apply_plan(paths: &HubPaths, plan: &BuiltinPlan<'_>) -> Result<Vec<PlanStep>, HubError> {
    let actions = plan.actions(paths);
    match plan {
        BuiltinPlan::Keep => {}
        BuiltinPlan::Create { skill, key } | BuiltinPlan::Overwrite { skill, key } => {
            write_skill(paths, key, skill)?;
        }
        BuiltinPlan::Remove { key } => {
            let dir = paths.skill_workdir(key);
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
                // 删除不可逆：键落 INFO，事后能重建删了哪个内置技能
                tracing::info!(key = %key, "内置技能移除完成");
            }
        }
    }
    Ok(actions)
}

/// 整份覆盖写入一个内置技能的工作副本：先清空目录再按内嵌文件落盘
fn write_skill(paths: &HubPaths, key: &str, skill: &BuiltinSkill) -> Result<(), HubError> {
    let dir = paths.skill_workdir(key);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    for file in &skill.files {
        let target = dir.join(&file.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, &file.contents)?;
    }
    Ok(())
}
