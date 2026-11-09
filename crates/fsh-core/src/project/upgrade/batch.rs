//! 批量升级：扫出范围内全部「固定版本且落后」的登记记录，一次升到最新 tag。
//! 预检全量先于任何盘动作——任一记录不合格整批不动盘并点名那条记录与它的落点；
//! 实跑逐条执行、每条自身原子，任一条失败即停并报告已完成与未开始。
//! 跟最新、已是最新、归档技能与死记录一律跳过并逐条报出，死记录指向 gc。

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::UpgradePlan;
use super::prepare::prepare_upgrade;
use super::run::{execute, planned_outcome};
use crate::ErrorCode;
use crate::git;
use crate::hub::{self, ProjectRegistry, ProjectSkill, Scope};
use crate::lock::WriteToken;
use crate::paths::HubPaths;
use crate::project::target::dedupe_anchors;
use crate::project::{
    Anchor, ProjectError, Targets, UpgradeAllOutcome, UpgradeOutcome, UpgradeSkip, UpgradeStop,
    anchors_of, lock_and_open, open_repo,
};
use crate::skill::{SkillState, SkillStatus, skill_status};
use crate::version::Version;

/// 批量升级：范围内全部「固定版本且落后」的记录一次升到最新 tag。
/// scope 给 None 即扫全 hub（项目段 + 全局段），给了目标就只收窄；
/// dry_run = true 时预演：只读不落盘、不取写锁，逐条给出动作清单。
pub fn upgrade_all(
    hub_root: &Path,
    scope: Option<Targets>,
    force: bool,
    dry_run: bool,
) -> Result<UpgradeAllOutcome, ProjectError> {
    let started = Instant::now();
    if dry_run {
        // 预演只读：不取写锁，别人正在写也照样出清单
        let (repo, paths) = open_repo(hub_root)?;
        return run_batch(&repo, &paths, scope, force, started, None);
    }
    // 写锁在整批期间一直握着：预检与执行看的是同一份盘面，两者之间没有窗口
    let (token, repo, paths) = lock_and_open(hub_root)?;
    run_batch(&repo, &paths, scope, force, started, Some(token))
}

/// 一批的执行主体：定集合 → 全量预检 → 逐条执行。
/// token 为空即预演，此时只有读动作，预检结果直接当报告
fn run_batch(
    repo: &git::Repository,
    paths: &HubPaths,
    scope: Option<Targets>,
    force: bool,
    started: Instant,
    token: Option<WriteToken>,
) -> Result<UpgradeAllOutcome, ProjectError> {
    let registry = hub::load_registry(paths)?;
    let scope = match scope {
        Some(targets) => Some(dedupe_anchors(anchors_of(&targets))?),
        None => None,
    };
    let scan = scan(repo, paths, &registry, scope.as_deref())?;
    let skipped = scan.skipped;

    // 预检全量先于任何盘动作：逐条按单条规则准备一遍，任一不合格整批不动盘，
    // 报错点名那条记录与它的落点，底层真因与稳定码原样透出
    let mut plans: Vec<(Candidate, UpgradePlan)> = Vec::with_capacity(scan.candidates.len());
    for candidate in scan.candidates {
        let plan = prepare_upgrade(
            repo,
            paths,
            &candidate.key,
            Some(&candidate.latest),
            vec![candidate.anchor.clone()],
            force,
        )
        .map_err(|cause| ProjectError::BatchPrecheck {
            key: candidate.key.clone(),
            target: candidate.anchor.label(),
            cause: Box::new(cause),
        })?;
        plans.push((candidate, plan));
    }

    let Some(token) = token else {
        // 预演：预检结果就是报告，动作清单已在计划里
        return Ok(UpgradeAllOutcome {
            dry_run: true,
            upgraded: plans
                .into_iter()
                .map(|(candidate, plan)| planned_outcome(&candidate.key, plan))
                .collect(),
            skipped,
            stopped: None,
        });
    };

    // 实跑：逐条执行，每条自身原子；失败即停，剩下的按原样报成未开始
    let mut upgraded: Vec<UpgradeOutcome> = Vec::with_capacity(plans.len());
    let mut stopped = None;
    let mut queue = plans.into_iter();
    while let Some((candidate, plan)) = queue.next() {
        match execute(&token, repo, paths, &candidate.key, plan) {
            Ok(outcome) => upgraded.push(outcome),
            Err(error) => {
                let stop = UpgradeStop {
                    key: candidate.key.clone(),
                    scope: candidate.anchor.scope(),
                    project: candidate.anchor.project(),
                    code: error.error_code().to_string(),
                    message: error.to_string(),
                    not_started: queue.map(|(pending, _)| pending.label()).collect(),
                };
                // 批量中止是这个边界的失败事件：点名停在哪条、已完成几条、共花了多久
                tracing::warn!(
                    key = %stop.key,
                    count = upgraded.len(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    cause = %stop.message,
                    "批量升级失败"
                );
                stopped = Some(stop);
                break;
            }
        }
    }
    // 整批完成是业务里程碑：一次调用一条，逐条里程碑已在执行段记过
    if stopped.is_none() {
        tracing::info!(
            count = upgraded.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "批量升级完成"
        );
    }
    Ok(UpgradeAllOutcome {
        dry_run: false,
        upgraded,
        skipped,
        stopped,
    })
}

/// 一条待升级的记录：一个目标、一个技能键、一个升到的版本
struct Candidate {
    /// 记录所属目标
    anchor: Anchor,
    /// 技能键
    key: String,
    /// 该技能的最新 tag：批量一律升到它
    latest: String,
}

impl Candidate {
    /// 记录人读名：`<键> @ <目标>`
    fn label(&self) -> String {
        format!("{} @ {}", self.key, self.anchor.label())
    }
}

/// 扫描结果：待升级的候选与跳过清单
struct Scan {
    /// 待升级的候选，全局记录在最前、项目记录按项目路径序
    candidates: Vec<Candidate>,
    /// 没纳入批量的记录与原因
    skipped: Vec<UpgradeSkip>,
}

/// 扫描范围内的登记记录，逐条定「升 / 不升」。
/// 记录按全局段在前、项目段按路径序排列，读报告与读登记簿同序
fn scan(
    repo: &git::Repository,
    paths: &HubPaths,
    registry: &ProjectRegistry,
    scope: Option<&[Anchor]>,
) -> Result<Scan, ProjectError> {
    let mut scan = Scan {
        candidates: Vec::new(),
        skipped: Vec::new(),
    };
    // 最新 tag 逐键只查一次：同一个技能可能被多处登记
    let mut statuses: BTreeMap<String, SkillStatus> = BTreeMap::new();
    if in_scope(scope, &Anchor::Global) {
        for skill in &registry.global().skills {
            classify(
                repo,
                paths,
                &mut statuses,
                &mut scan,
                &Anchor::Global,
                skill,
            )?;
        }
    }
    for (path, record) in &registry.projects {
        let dir = PathBuf::from(path);
        let anchor = Anchor::Project(dir.clone());
        if !in_scope(scope, &anchor) {
            continue;
        }
        // 死记录：项目目录已不存在，整条记录归 gc 收拾，这里只点名并跳过
        if !dir.is_dir() {
            let skip = UpgradeSkip {
                key: None,
                scope: Scope::Project,
                project: anchor.project(),
                version: None,
                reason: "项目目录已不存在，死记录用 fsh gc 收拾".to_string(),
            };
            log_skip(&skip);
            scan.skipped.push(skip);
            continue;
        }
        for skill in &record.skills {
            classify(repo, paths, &mut statuses, &mut scan, &anchor, skill)?;
        }
    }
    Ok(scan)
}

/// 一条记录的判定结果
enum Verdict {
    /// 固定版本且落后：升到给出的最新 tag
    Stale(String),
    /// 不升：为什么
    Skip(String),
}

/// 判定一条记录升不升：只看技能自己的三态与登记版本，
/// 与它登记在哪个目标无关，因此目标只用于报告
fn verdict_of(
    repo: &git::Repository,
    paths: &HubPaths,
    statuses: &mut BTreeMap<String, SkillStatus>,
    skill: &ProjectSkill,
) -> Result<Verdict, ProjectError> {
    let key = skill.key.as_str();
    let status = status_of(repo, paths, statuses, key)?;
    match status.state() {
        // tag 与工作副本都没了：技能已不在库里
        SkillState::Missing => Ok(Verdict::Skip(
            "技能不在库里，既无工作副本也无版本 tag".to_string(),
        )),
        // 归档技能：工作副本不在，换靶没有源
        SkillState::Archived => Ok(Verdict::Skip("技能已归档（先 fsh restore）".to_string())),
        SkillState::Active => {
            let Some(from) = skill.version.as_deref() else {
                // 跟最新：链接已跟随工作副本；拷贝是快照，是否落后无法判定
                return Ok(Verdict::Skip(
                    "跟最新，拷贝落点是否落后无法判定".to_string(),
                ));
            };
            // 最新版本取版本清单表头——全仓最新版本的取法只有这一处
            let Some(to) = status.latest() else {
                return Ok(Verdict::Skip(
                    "技能没有任何版本 tag，无法判定落后".to_string(),
                ));
            };
            let to = to.to_string();
            // 落后判定：登记版本与最新 tag 都能解析，且登记版本更旧。判不出来的一律不当落后，
            // 批量不动一条判不出所以然的记录
            let (Some(old), Some(newest)) = (Version::parse(from), Version::parse(&to)) else {
                return Ok(Verdict::Skip(format!(
                    "登记版本 {from} 与最新 tag {to} 比不出大小，判不出落后"
                )));
            };
            if old == newest {
                return Ok(Verdict::Skip(format!("已是该技能的最新版本（@{to}）")));
            }
            if old > newest {
                return Ok(Verdict::Skip(format!(
                    "登记版本 {from} 高于最新 tag（@{to}），不纳入批量"
                )));
            }
            Ok(Verdict::Stale(to))
        }
    }
}

/// 记录判定结果：落后进候选，其余进跳过清单并留一条 WARN
fn classify(
    repo: &git::Repository,
    paths: &HubPaths,
    statuses: &mut BTreeMap<String, SkillStatus>,
    scan: &mut Scan,
    anchor: &Anchor,
    skill: &ProjectSkill,
) -> Result<(), ProjectError> {
    match verdict_of(repo, paths, statuses, skill)? {
        Verdict::Stale(to) => scan.candidates.push(Candidate {
            anchor: anchor.clone(),
            key: skill.key.clone(),
            latest: to,
        }),
        Verdict::Skip(reason) => scan.record_skip(anchor, skill, reason),
    }
    Ok(())
}

impl Scan {
    /// 记一条技能记录的跳过：报告与日志同源，日志里能 grep 到「批量升级跳过」
    fn record_skip(&mut self, anchor: &Anchor, skill: &ProjectSkill, reason: impl Into<String>) {
        let skip = UpgradeSkip {
            key: Some(skill.key.clone()),
            scope: anchor.scope(),
            project: anchor.project(),
            version: skill.version.clone(),
            reason: reason.into(),
        };
        log_skip(&skip);
        self.skipped.push(skip);
    }
}

/// 跳过是决策点：每条没纳入批量的记录各记一条 WARN，
/// 读日志即可回答「为什么没升它」；原因进 `cause`，其余字段与里程碑同形
fn log_skip(skip: &UpgradeSkip) {
    match &skip.project {
        Some(project) => tracing::warn!(
            key = %skip.key.as_deref().unwrap_or_default(),
            version = %skip.version.as_deref().unwrap_or_default(),
            project = %project,
            cause = %skip.reason,
            "批量升级跳过"
        ),
        None => tracing::warn!(
            key = %skip.key.as_deref().unwrap_or_default(),
            version = %skip.version.as_deref().unwrap_or_default(),
            scope = "global",
            cause = %skip.reason,
            "批量升级跳过"
        ),
    }
}

/// 某技能的状态一次取全并缓存：同一技能可能被多处登记，一次扫描只查一次
fn status_of<'a>(
    repo: &git::Repository,
    paths: &HubPaths,
    cache: &'a mut BTreeMap<String, SkillStatus>,
    key: &str,
) -> Result<&'a SkillStatus, ProjectError> {
    Ok(match cache.entry(key.to_string()) {
        Entry::Occupied(hit) => hit.into_mut(),
        Entry::Vacant(slot) => slot.insert(skill_status(repo, paths, key)?),
    })
}

/// 目标是否在范围内：没有范围即全收，有范围按目标身份比对
fn in_scope(scope: Option<&[Anchor]>, anchor: &Anchor) -> bool {
    let Some(scope) = scope else { return true };
    scope.iter().any(|other| other.same_as(anchor))
}
