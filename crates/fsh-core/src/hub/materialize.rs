//! 内置技能对齐：把编译期内嵌的技能表整份物化进 `store\builtin\`，
//! 让 `fsh init` 之后内置技能就是库里一个普通技能。
//! 对齐以程序内嵌内容为准：键缺失整份落地、内容不同整份覆盖、
//! 内容相同一个字节不动、内嵌里已无的键整份删掉。
//! 对齐只交计划与落盘两件事，锁与提交归 init 那一次装配骨架。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::HubError;
use crate::bundled::BuiltinSkill;
use crate::commit::{Face, Verb, message};
use crate::naming::{BUILTIN_PREFIX, builtin_key};
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::skill::skill_occupancy;
use crate::write::Ctx;

/// 一个内置技能的对齐计划：动作与执行所需数据同体
pub(crate) enum BuiltinPlan<'a> {
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
    /// 该计划在盘上的动作描述：预演直接列它，实跑按同一份清单落盘。
    /// 提交步归整份计划——一次对齐只提交一次
    fn desc(&self) -> Option<PlanStep> {
        match self {
            BuiltinPlan::Create { key, .. } | BuiltinPlan::Overwrite { key, .. } => Some(
                PlanStep::of(ActionKind::Write, crate::naming::store_subpath(key)),
            ),
            BuiltinPlan::Keep => None,
            BuiltinPlan::Remove { key } => Some(PlanStep::of(
                ActionKind::Remove,
                crate::naming::store_subpath(key),
            )),
        }
    }

    /// 这条计划是否要动盘：内容一致的 Keep 不动，里程碑按它数动作数
    pub(crate) fn described(&self) -> bool {
        self.desc().is_some()
    }
}

/// 整份对齐计划：内嵌表顺序即执行顺序（目录名升序），孤儿键升序。
/// 逐技能动作之后是末尾一条提交步——只在有动作时出现，内容一致的对齐不提交
pub(crate) fn builtin_plan<'a>(
    paths: &HubPaths,
    skills: &'a [BuiltinSkill],
) -> Result<Plan<Vec<BuiltinPlan<'a>>>, HubError> {
    let orphans = {
        let embedded: BTreeSet<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
        orphan_names(paths, &embedded)?
    };
    let mut data: Vec<BuiltinPlan<'a>> = skills
        .iter()
        .map(|skill| plan_skill(paths, skill))
        .collect();
    data.extend(orphans.into_iter().map(|name| BuiltinPlan::Remove {
        key: builtin_key(&name),
    }));
    let mut steps: Vec<PlanStep> = data.iter().filter_map(BuiltinPlan::desc).collect();
    if !steps.is_empty() {
        steps.push(PlanStep::commit(message(
            Verb::Init,
            Face::Builtin,
            "builtin",
        )));
    }
    let mut plan = Plan::new(data);
    for desc in steps {
        plan.push(desc, Act::None);
    }
    Ok(plan)
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

/// `store\builtin\` 下不在内嵌表里的目录名（升序）：内嵌里已无的技能目录。
/// 对齐把每个都整份删掉，不留孤儿
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

/// 实跑整份计划：只落盘，提交由整份计划统一收口
pub(crate) fn land_builtin(ctx: &mut Ctx<'_>, plans: &[BuiltinPlan<'_>]) -> Result<(), HubError> {
    for plan in plans {
        match plan {
            BuiltinPlan::Keep => {}
            BuiltinPlan::Create { skill, key } | BuiltinPlan::Overwrite { skill, key } => {
                write_skill(ctx, key, skill)?;
            }
            BuiltinPlan::Remove { key } => {
                let dir = ctx.paths().skill_workdir(key);
                if dir.exists() {
                    ctx.remove_dir(&dir)?;
                    // 删除不可逆：键落 INFO，事后能重建删了哪个内置技能
                    tracing::info!(key = %key, "内置技能移除完成");
                }
            }
        }
    }
    Ok(())
}

/// 整份覆盖写入一个内置技能的工作副本：先清空目录再按内嵌文件落盘
fn write_skill(ctx: &mut Ctx<'_>, key: &str, skill: &BuiltinSkill) -> Result<(), HubError> {
    let dir = ctx.paths().skill_workdir(key);
    ctx.remove_dir(&dir)?;
    ctx.create_dir(&dir)?;
    for file in &skill.files {
        ctx.write_file(&dir.join(&file.path), &file.contents)?;
    }
    Ok(())
}
