//! 入库：本地技能目录一步到位成为库内实体（工作副本 + 自动提交）。
//! 一次入库一整套：源发现按两层判定（见 [`discover`]），键由前缀与发现到的
//! 名字拼出；键已存在的进跳过清单，其余照常入库，整批只留一条自动提交。

use std::fs;
use std::path::{Path, PathBuf};

use super::discover::{self, Found};
use super::frontmatter::parse;
use super::outcome::{KeySet, SelectionOutcome, Skipped, dry};
use super::{Frontmatter, SkillError, invalid, skill_occupancy, walk_face};
use crate::commit::{Face, Verb, batch_message};
use crate::naming;
use crate::paths::HubPaths;
use crate::plan::{Act, ActionKind, Plan, PlanStep};
use crate::select::Selector;
use crate::write::{self, Applied, OnFailure};

/// 入库一整套：按两层规则发现源目录里的技能，选择器筛出要入的那些，
/// 键已存在的跳过并报告，其余整目录复制进 store，最后一条机器提交。
/// dry_run = true 时预演：只读不落盘、不取写锁。
pub fn add(
    hub_root: &Path,
    src_dir: &Path,
    selector: &Selector,
    dry_run: bool,
) -> Result<SelectionOutcome, SkillError> {
    // 预检先于开仓：源发现、frontmatter、键形状与「键长在已有技能的内容树里」
    // 都在这里一次收齐，坏入参不落到「hub 未初始化」，预演与实跑同序
    let picked = pick(selector, src_dir, &HubPaths::from_root(hub_root))?;
    write::run(
        hub_root,
        dry_run,
        // 复制出的工作副本要撤干净：盘上不留没有入库记录的半棵树
        OnFailure::Rollback,
        |paths| prepare_add(paths, &picked),
        dry,
        |_ctx, plan| {
            let KeySet { applied, skipped } = plan.data;
            let count = applied.len();
            Ok(Applied {
                outcome: SelectionOutcome {
                    applied,
                    skipped,
                    verb: Some(Verb::Add),
                    ..SelectionOutcome::default()
                },
                milestones: count,
            })
        },
        |count, elapsed_ms| {
            // 入库跨复制目录与机器提交两段外部调用：一条 INFO 交代整套、条数与整段耗时
            tracing::info!(
                prefix = %selector.label(),
                count,
                elapsed_ms = elapsed_ms,
                "skill 入库完成"
            );
        },
    )
}

/// 要入库的一项：键与它的源目录
#[derive(Debug, Clone)]
struct Picked {
    /// 完整键（前缀 + 技能名）
    key: String,
    /// 源目录
    dir: PathBuf,
}

/// 预检段：发现源目录里的技能 → 选择器筛出要入的 → 逐个读身份与键形状、
/// 查键与既有技能的内容树冲突。坏项一次收齐后统一报出，整批不入库；
/// 全部通过才返回要入库的清单
fn pick(selector: &Selector, src_dir: &Path, paths: &HubPaths) -> Result<Vec<Picked>, SkillError> {
    // 键形状先判：写错的前缀与技能名当场点名，不落到盘面判定
    selector.check()?;
    let found = discover::discover(src_dir)?;
    let available = keys_of(&found, selector.prefix());
    let keys = selector.resolve(&available)?;
    // 未表态（既没点名也没说全要）：源里恰好一个技能才成立
    let keys = if keys.is_empty() {
        if available.len() > 1 {
            return Err(SkillError::AmbiguousSource {
                count: available.len(),
                names: available.join("、"),
            });
        }
        available
    } else {
        keys
    };

    let mut problems: Vec<String> = Vec::new();
    let mut picked: Vec<Picked> = Vec::new();
    for key in keys {
        let name = crate::key::last_segment(&key);
        let Some(skill) = found.iter().find(|f| f.name == name) else {
            // 选择器只可能点到发现到的名字，走到这里说明两个清单不一致
            continue;
        };
        let before = problems.len();
        if let Err(e) = identity(&skill.dir, &skill.name, key.clone()) {
            problems.push(e.to_string());
        }
        // 键不得长在别的键的内容树里：祖先段已是技能时两棵目录树重叠，
        // 键隔离失效；store 与 archive 两个面都查
        if let Some(ancestor) = ancestor_skill(paths, &key) {
            problems.push(format!("键 {key} 落在已有键 {ancestor} 的内容树里"));
        }
        if problems.len() == before {
            picked.push(Picked {
                key,
                dir: skill.dir.clone(),
            });
        }
    }
    if !problems.is_empty() {
        return Err(invalid(
            &src_dir.display().to_string(),
            format!(
                "{} 个技能不成立，整批不入库: {}",
                problems.len(),
                problems.join("；")
            ),
        ));
    }
    Ok(picked)
}

/// 发现结果拼成键：键 = 前缀 + 技能名，按名字排序（发现已排序）
fn keys_of(found: &[Found], prefix: Option<&str>) -> Vec<String> {
    found
        .iter()
        .map(|f| match prefix {
            Some(prefix) => format!("{prefix}/{}", f.name),
            None => f.name.clone(),
        })
        .collect()
}

/// 键的祖先段是否已是技能：store 与 archive 两个面的键空间同构，两面都查，
/// 命中即返回那个祖先键
fn ancestor_skill(paths: &HubPaths, key: &str) -> Option<String> {
    let faces = [paths.store(), paths.archive()];
    faces
        .iter()
        .flat_map(|face| walk_face(face))
        .find(|skill| skill.has_skill_md && key.starts_with(&format!("{}/", skill.key)))
        .map(|skill| skill.key)
}

/// 身份约束：frontmatter name 必须等于它所在目录名（键末段）。
/// 入库判据只有这一处，判断与文案只有这一份
fn identity(dir: &Path, dir_name: &str, key: String) -> Result<(), SkillError> {
    let skill_md = dir.join("SKILL.md");
    let label = skill_md.display().to_string();
    let text = fs::read_to_string(&skill_md).map_err(|e| SkillError::InvalidSource {
        path: dir.display().to_string(),
        reason: format!("读不到 SKILL.md: {e}"),
    })?;
    let fm = parse(&text, &label)?;
    ensure_name_matches_dir(&fm, dir_name, &label)?;
    // 键段是被 git 与文件系统共同寻址的相对路径片段：形状规则由 key 模块统一判定
    crate::key::validate(&key)?;
    Ok(())
}

/// frontmatter name 与所在目录名一致：键末段与目录事实一致，生态递归发现才成立
fn ensure_name_matches_dir(
    frontmatter: &Frontmatter,
    dir_name: &str,
    label: &str,
) -> Result<(), SkillError> {
    if frontmatter.name == dir_name {
        return Ok(());
    }
    Err(invalid(
        label,
        format!(
            "frontmatter name ({}) 与所在目录名 ({dir_name}) 不一致",
            frontmatter.name
        ),
    ))
}

/// add 预检（hub 侧）：键已存在的跳过并报告，其余进复制步骤，整批一条提交。
/// 键与既有技能的内容树冲突已在预检段一次收齐，这里只剩占位判定
fn prepare_add(paths: &HubPaths, picked: &[Picked]) -> Result<Plan<KeySet>, SkillError> {
    let mut plan = Plan::new(KeySet::default());
    for item in picked {
        // 重键不覆盖：键在场（工作副本或归档副本占位）进跳过清单，其余照常入库
        let occupancy = skill_occupancy(paths, &item.key);
        if let Err(e) = occupancy.require_vacant() {
            plan.data
                .skipped
                .push(Skipped::of(&item.key, &SkillError::from(e)));
            continue;
        }
        plan.data.applied.push(item.key.clone());
        plan.push(
            PlanStep::between(
                ActionKind::Copy,
                item.dir.display().to_string(),
                naming::store_subpath(&item.key),
            )
            .targeting(&item.key),
            Act::Copy {
                from: item.dir.clone(),
                to: paths.skill_workdir(&item.key),
            },
        );
    }
    // 一条都没做成就没有提交步：计划里没有提交步即不产生自动提交
    if !plan.data.applied.is_empty() {
        let commit = batch_message(Verb::Add, &[Face::Store], &plan.data.applied);
        plan.push(PlanStep::commit(commit), Act::None);
    }
    Ok(plan)
}
