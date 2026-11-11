//! 内置技能对齐：把编译期内嵌的技能表整份物化进 `store\builtin\`，
//! 让 `fsh init` 之后内置技能就是库里一个普通技能。
//! 对齐以程序内嵌内容为准：键缺失整份落地、内容不同整份覆盖、
//! 内容相同一个字节不动、内嵌里已无的键整份删掉。

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use super::HubError;
use crate::builtin::BuiltinSkill;
use crate::git;
use crate::naming::{BUILTIN_PREFIX, builtin_key};
use crate::paths::HubPaths;
use crate::skill::{SkillState, skill_status};

/// init 的收尾一步：把 `store\builtin\` 与内嵌技能表整份对齐。
/// 键由 store 内相对路径得出（`builtin/<目录名>`），不另造键、不做名字映射；
/// dry_run = true 时只算动作清单，不落盘、不取写锁
pub(crate) fn sync_builtin(
    paths: &HubPaths,
    skills: &[BuiltinSkill],
    dry_run: bool,
) -> Result<Vec<String>, HubError> {
    if dry_run {
        return plan_builtin(paths, skills);
    }
    let started = Instant::now();
    // 内置技能落地也是 hub 写操作：与其他写入口共用同一把全局写锁
    let _token = crate::lock::write_lock(paths)?;
    let repo = git::open_hub(paths.root())?;
    let mut actions = Vec::new();
    for plan in plan_builtin_with(paths, Some(&repo), skills)? {
        actions.extend(apply_plan(paths, &plan)?);
    }
    if actions.is_empty() {
        // 内容一致是幂等命中：重跑 init 的正常结果，DEBUG 留痕不刷屏
        tracing::debug!(count = skills.len(), "内置技能内容一致，对齐跳过");
    } else {
        // 对齐有落盘：一次机器提交收编全部内置技能变更
        git::auto_commit(&repo, "init(builtin): builtin");
        tracing::info!(
            path = %paths.root().display(),
            count = actions.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "内置技能对齐完成"
        );
    }
    Ok(actions)
}

/// 预演：只读算出内置技能对齐的动作清单
fn plan_builtin(paths: &HubPaths, skills: &[BuiltinSkill]) -> Result<Vec<String>, HubError> {
    let repo = open_repo_if_any(paths)?;
    Ok(plan_builtin_with(paths, repo.as_ref(), skills)?
        .iter()
        .flat_map(|plan| plan.actions(paths))
        .collect())
}

/// 已装配的 hub 开只读仓库；全新装配（尚无 .git）没有仓库可读
fn open_repo_if_any(paths: &HubPaths) -> Result<Option<git::Repository>, HubError> {
    if !paths.root().join(".git").exists() {
        return Ok(None);
    }
    let repo = git::Repository::open(paths.root()).map_err(git::GitError::from)?;
    Ok(Some(repo))
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
    /// 该计划的动作清单：预演直接输出，实跑按同一份清单执行
    fn actions(&self, paths: &HubPaths) -> Vec<String> {
        match self {
            BuiltinPlan::Create { key, .. } | BuiltinPlan::Overwrite { key, .. } => vec![
                format!("write {}", crate::naming::store_subpath(key)),
                "commit: init(builtin): builtin".to_string(),
            ],
            BuiltinPlan::Keep => Vec::new(),
            BuiltinPlan::Remove { key } => {
                if paths.skill_workdir(key).exists() {
                    vec![
                        format!("remove {}", crate::naming::store_subpath(key)),
                        "commit: init(builtin): builtin".to_string(),
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
    repo: Option<&git::Repository>,
    skills: &'a [BuiltinSkill],
) -> Result<Vec<BuiltinPlan<'a>>, HubError> {
    let mut plans: Vec<BuiltinPlan<'a>> = skills
        .iter()
        .map(|skill| plan_skill(paths, repo, skill))
        .collect::<Result<_, _>>()?;
    plans.extend(builtin_orphans(paths, skills)?);
    Ok(plans)
}

/// 单个内置技能的计划：键缺失落地、内容不同覆盖、内容相同不动
fn plan_skill<'a>(
    paths: &HubPaths,
    repo: Option<&git::Repository>,
    skill: &'a BuiltinSkill,
) -> Result<BuiltinPlan<'a>, HubError> {
    let key = builtin_key(&skill.name);
    let status = match repo {
        Some(repo) => Some(skill_status(repo, paths, &key)?),
        None => None,
    };
    let action = match status.as_ref().map(|status| status.state()) {
        // 无仓库（全新装配）与键缺失同路：整份落地
        None | Some(SkillState::Missing) => BuiltinPlan::Create { skill, key },
        Some(SkillState::Active) if workdir_matches(paths, &key, skill) => BuiltinPlan::Keep,
        Some(_) => BuiltinPlan::Overwrite { skill, key },
    };
    Ok(action)
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

/// 实跑一个计划：只落盘，提交由 sync_builtin 在全部计划执行完统一收口
fn apply_plan(paths: &HubPaths, plan: &BuiltinPlan<'_>) -> Result<Vec<String>, HubError> {
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
