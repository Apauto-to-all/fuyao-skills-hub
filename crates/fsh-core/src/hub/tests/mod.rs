//! hub 目录模块的测试：按被测概念分文件，共享夹具集中在这里。

use std::fs;

use crate::bundled::{BuiltinFile, BuiltinSkill};
use crate::git;
use crate::hub::{HubConfig, load_config};
use crate::paths::HubPaths;
use crate::plan::PlanStep;
use crate::write::{self, Applied};

mod config;
mod init;
mod materialize;
mod registry;

/// 造一个就绪的 hub：内部走对外入口 init（目录树 + 配置 + 仓库 + 首提交 +
/// 上架内置技能），返回临时目录与路径族。临时目录要绑到具名变量上——
/// 只写 `_` 会在语句结束时就把它删掉，hub 根随之消失
pub(super) fn setup() -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    crate::hub::init(install.path(), false).unwrap();
    let paths = HubPaths::from_root(install.path());
    (install, paths)
}

/// 就绪且 `store\builtin\` 为空的 hub：注入自造内嵌表的对齐测试要这个起点——
/// 对齐把「不在给定内嵌表里的键」当孤儿整份清掉，真实内置技能在场会
/// 混进动作清单。清空走 skill::remove（对外入口），剩下的空目录壳删掉
pub(super) fn setup_with_empty_builtin() -> (tempfile::TempDir, HubPaths) {
    let (install, paths) = setup();
    for skill in crate::bundled::skills() {
        let selector = crate::select::Selector::named(
            Some(crate::naming::BUILTIN_PREFIX.to_string()),
            vec![skill.name.clone()],
        );
        crate::skill::remove(paths.root(), &selector, false).unwrap();
    }
    // 上架目录只剩空壳：git 不跟踪空目录，删掉不影响工作区干净状态
    fs::remove_dir(paths.store().join("builtin")).unwrap();
    (install, paths)
}

/// 注入自造内嵌表的对齐：排计划、落盘、提交与 init 收尾同一段落盘，
/// 只是内嵌表由用例给定。dry_run 只交计划，不落盘、不取写锁
pub(super) fn land_skills(
    paths: &HubPaths,
    skills: &[BuiltinSkill],
    dry_run: bool,
) -> Result<Vec<PlanStep>, crate::hub::HubError> {
    write::run_assembly(
        paths.root(),
        dry_run,
        |paths| super::materialize::builtin_plan(paths, skills),
        // 这些用例守的是对齐计划本身，骨架的手改快照步不在清单里
        |plan, _command: Vec<PlanStep>| plan.describe(),
        |ctx, plan| {
            super::materialize::land_builtin(ctx, &plan.data)?;
            // 真正落盘的步骤就是计划里的动作：内容一致的对齐没有步骤
            let actions = plan.describe();
            Ok(Applied {
                outcome: actions,
                milestones: (),
            })
        },
        |_: (), _elapsed| {},
    )
}

/// 断言一条龙初始化后的完整落盘状态
pub(super) fn assert_initialized(paths: &HubPaths) {
    // 目录树齐全
    for face in crate::paths::FACES {
        let dir = paths.root().join(face);
        assert!(dir.is_dir(), "缺少目录: {dir:?}");
    }
    // .gitignore 是白名单：先忽略根下一切，再放行固定两项与全部 tracked 面
    let gi = fs::read_to_string(paths.gitignore()).unwrap();
    assert_eq!(
        gi, "/*\n!/.gitignore\n!/hub.toml\n!/agents/\n!/store/\n!/archive/\n",
        "白名单正文逐字固定：条目、顺序与换行都不许漂"
    );
    // hub.toml 可读回且是首版默认值
    let cfg = load_config(paths).unwrap().expect("hub.toml 应存在");
    assert_eq!(cfg, HubConfig::v1());
    // git 仓库存在、main 分支、关 autocrlf、工作区干净（首提交已完成）
    let repo = git2::Repository::open(paths.root()).unwrap();
    let head = repo.find_reference("HEAD").unwrap();
    assert_eq!(
        head.symbolic_target().unwrap(),
        Some("refs/heads/main"),
        "初始分支应为 main"
    );
    assert_eq!(
        repo.config().unwrap().get_string("core.autocrlf").unwrap(),
        "false"
    );
    assert!(!git::is_dirty(&repo).unwrap(), "首提交后工作区应干净");
    // 全局写锁落在 hub 根下，且被 git 忽略：锁文件永远不进任何提交
    let lock = paths.root().join(".lock");
    assert!(lock.is_file(), "缺少全局写锁文件: {lock:?}");
    assert!(
        repo.status_should_ignore(std::path::Path::new(".lock"))
            .unwrap(),
        ".lock 必须被 git 忽略"
    );
    // 首提交是机器提交：固定身份写死在程序里
    let commit = repo.head().unwrap().peel_to_commit().unwrap();
    assert_eq!(commit.author().name().unwrap(), "fsh");
    assert_eq!(commit.author().email().unwrap(), "fsh@local");
}

/// 造一个内置技能表项：SKILL.md 按参数写名字与正文，
/// 另带一个子目录文件，覆盖内嵌表的递归遍历与物化建目录两条链路
pub(super) fn builtin_skill(name: &str, body: &str) -> BuiltinSkill {
    BuiltinSkill {
        name: name.to_string(),
        files: vec![
            BuiltinFile {
                path: "SKILL.md".to_string(),
                contents: format!("---\nname: {name}\ndescription: 演示\n---\n\n{body}\n")
                    .into_bytes(),
            },
            BuiltinFile {
                path: "references/a.txt".to_string(),
                contents: format!("ref:{body}").into_bytes(),
            },
        ],
    }
}

/// 当前 HEAD 提交 id（尚无提交时为 None）
pub(super) fn head_oid(paths: &HubPaths) -> Option<git::Oid> {
    let repo = git2::Repository::open(paths.root()).unwrap();
    repo.head().ok().and_then(|h| h.target())
}
