//! 项目面测试：按被操作分组，共享夹具住在本文件。
//! 各测试只经项目使用与撤回的公开操作、登记簿和磁盘落点观察行为。

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::hub;

/// 建一次性 hub（初始化完成）+ 一个已入库技能
pub(super) fn setup_with_skill(version: Option<&str>) -> (tempfile::TempDir, HubPaths) {
    let install = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let src = src_root.path().join("demo");
    fs::create_dir_all(src.join("references")).unwrap();
    let fm_version = version
        .map(|v| format!("metadata:\n  version: \"{v}\"\n"))
        .unwrap_or_default();
    fs::write(
        src.join("SKILL.md"),
        format!("---\nname: demo\ndescription: 演示\n{fm_version}---\n\n正文\n"),
    )
    .unwrap();
    fs::write(src.join("references/a.txt"), "ref-a").unwrap();
    crate::skill::add(paths.root(), &src, None, false).unwrap();
    (install, paths)
}

/// 造两个共用同一项目技能目录的用户 agent：它们的落点会合并成一处
pub(super) fn twin_agents(paths: &HubPaths) -> Vec<String> {
    let keys = ["twin-a".to_string(), "twin-b".to_string()];
    fs::create_dir_all(paths.agents()).unwrap();
    for key in &keys {
        fs::write(
            paths.agents().join(format!("{key}.toml")),
            "skills_dir = \".agents/skills\"\n",
        )
        .unwrap();
    }
    keys.to_vec()
}

/// 建一个有项目目录的一次性项目（项目根必须已存在；不自动删，路径留给测试用）
pub(super) fn temp_project() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.keep();
    fs::create_dir_all(&p).unwrap();
    p
}

/// 一次 use 的 agent 列表（通用位 + dsh 两个不同落点）
pub(super) fn two_agents() -> Vec<String> {
    vec!["universal".to_string(), "dsh".to_string()]
}

/// 造一个只有全局目录的用户 agent：全局落点落在给定目录里，不碰真实家目录
pub(super) fn global_agent(paths: &HubPaths, dir: &Path) -> String {
    global_agent_as(paths, "globalonly", dir)
}

/// 同上，键由调用方给：需要两个全局位 agent 时各给一个键
pub(super) fn global_agent_as(paths: &HubPaths, key: &str, dir: &Path) -> String {
    fs::create_dir_all(paths.agents()).unwrap();
    fs::write(
        paths.agents().join(format!("{key}.toml")),
        format!(
            "display_name = \"只有全局位\"\nglobal_skills_dir = {:?}\n",
            dir.display().to_string()
        ),
    )
    .unwrap();
    key.to_string()
}

/// 登记簿里某项目的该技能记录
pub(super) fn record(paths: &HubPaths, project: &Path, key: &str) -> Option<hub::ProjectSkill> {
    let reg = hub::load_registry(paths).unwrap();
    reg.project(project)
        .and_then(|p| p.skills.iter().find(|s| s.key == key).cloned())
}

/// 落点是不是指向这个源：链接档比规范化靶子，拷贝档比正文内容。
/// 版本切换不改技能内容，所以拷贝档也能用「读到的是不是同一份正文」判定
pub(super) fn links_to(dest: &Path, source: &Path, kind: LinkKind) -> bool {
    match kind {
        LinkKind::Copy => {
            let body = |p: &Path| fs::read_to_string(p.join("SKILL.md")).unwrap_or_default();
            body(dest) == body(source)
        }
        LinkKind::Symlink | LinkKind::Junction => fs::canonicalize(dest)
            .ok()
            .zip(fs::canonicalize(source).ok())
            .is_some_and(|(a, b)| a == b),
    }
}

/// 登记簿文件原文：一字不动是整批还原、幂等命中、预演与预检拒绝的判据
pub(super) fn registry_text(paths: &HubPaths) -> String {
    fs::read_to_string(paths.projects_toml()).unwrap_or_default()
}

/// 按 agent 撤回的目标侧：两侧只有目标与落点位置不同，判定完全共用
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Project,
    Global,
}

/// 一次按 agent 撤回的用例现场：两个 agent 与它们各自的落点
pub(super) struct Partial {
    /// hub 的一次性安装
    _install: tempfile::TempDir,
    /// 全局侧各 agent 的落点根，须活到用例结束
    _roots: Vec<tempfile::TempDir>,
    /// 项目侧的项目根；全局侧没有
    project: Option<PathBuf>,
    paths: HubPaths,
    side: Side,
    agents: Vec<String>,
    dests: Vec<PathBuf>,
}

impl Partial {
    /// 两个 agent 各占一个落点
    pub(super) fn distinct(side: Side) -> Self {
        let (install, paths) = setup_with_skill(None);
        let (agents, dests, project, roots) = match side {
            Side::Project => {
                let project = temp_project();
                let dests = vec![
                    project.join(".agents/skills/demo"),
                    project.join(".dsh/skills/demo"),
                ];
                (two_agents(), dests, Some(project), Vec::new())
            }
            Side::Global => {
                let first = tempfile::tempdir().unwrap();
                let second = tempfile::tempdir().unwrap();
                let agents = vec![
                    global_agent_as(&paths, "g-one", first.path()),
                    global_agent_as(&paths, "g-two", second.path()),
                ];
                let dests = vec![first.path().join("demo"), second.path().join("demo")];
                (agents, dests, None, vec![first, second])
            }
        };
        Self {
            _install: install,
            _roots: roots,
            project,
            paths,
            side,
            agents,
            dests,
        }
    }

    /// 两个 agent 解析到同一个落点
    pub(super) fn shared(side: Side) -> Self {
        let (install, paths) = setup_with_skill(None);
        let (agents, dest, project, roots) = match side {
            Side::Project => {
                let project = temp_project();
                let agents = twin_agents(&paths);
                let dest = project.join(".agents/skills/demo");
                (agents, dest, Some(project), Vec::new())
            }
            Side::Global => {
                let root = tempfile::tempdir().unwrap();
                let agents = vec![
                    global_agent_as(&paths, "g-one", root.path()),
                    global_agent_as(&paths, "g-two", root.path()),
                ];
                let dest = root.path().join("demo");
                (agents, dest, None, vec![root])
            }
        };
        Self {
            _install: install,
            _roots: roots,
            project,
            paths,
            side,
            agents,
            dests: vec![dest.clone(), dest],
        }
    }

    /// 第 i 个 agent 的落点
    pub(super) fn dest(&self, i: usize) -> &Path {
        &self.dests[i]
    }

    /// 只撤第 i 个 agent 的名单
    pub(super) fn only(&self, i: usize) -> Vec<String> {
        vec![self.agents[i].clone()]
    }

    /// 这个现场的目标集合
    fn targets(&self) -> Targets {
        match self.side {
            Side::Project => Targets::project(self.project.as_ref().unwrap()),
            Side::Global => Targets::global(),
        }
    }

    /// 把两个 agent 一次铺到这个现场
    pub(super) fn use_all(&self) -> UseOutcome {
        use_skill(
            self.paths.root(),
            "demo",
            None,
            &self.agents,
            self.targets(),
            false,
        )
        .unwrap()
    }

    /// 在这个现场按名单撤回
    pub(super) fn unuse(
        &self,
        only: Option<&[String]>,
        force: bool,
        dry_run: bool,
    ) -> Result<UnuseOutcome, ProjectError> {
        unuse(
            self.paths.root(),
            "demo",
            self.targets(),
            only,
            force,
            dry_run,
        )
    }

    /// 记录里剩下的 agent；记录没了就是 None
    pub(super) fn record_agents(&self) -> Option<Vec<String>> {
        match self.side {
            Side::Project => record(&self.paths, self.project.as_ref().unwrap(), "demo")
                .map(|skill| skill.agents),
            Side::Global => hub::load_registry(&self.paths)
                .unwrap()
                .global()
                .skills
                .iter()
                .find(|skill| skill.key == "demo")
                .map(|skill| skill.agents.clone()),
        }
    }

    /// 这个现场还有没有登记记录
    pub(super) fn records_empty(&self) -> bool {
        let registry = hub::load_registry(&self.paths).unwrap();
        match self.side {
            Side::Project => registry.projects.is_empty(),
            Side::Global => registry.global().skills.is_empty(),
        }
    }

    /// 删掉第 i 个 agent 的配置：它已经解析不出来
    pub(super) fn remove_agent_config(&self, i: usize) {
        fs::remove_file(self.paths.agents().join(format!("{}.toml", self.agents[i]))).unwrap();
    }

    /// 把第 i 个 agent 改成只有另一侧的目录：这个作用域里解析不出落点
    pub(super) fn move_agent_to_other_side(&self, i: usize) {
        let config = match self.side {
            Side::Project => "global_skills_dir = \"~/.nowhere\"\n",
            Side::Global => "skills_dir = \".nowhere/skills\"\n",
        };
        fs::write(
            self.paths.agents().join(format!("{}.toml", self.agents[i])),
            config,
        )
        .unwrap();
    }

    /// 「该作用域没有目录」对应的错误码
    pub(super) fn no_side_error(&self) -> &'static str {
        match self.side {
            Side::Project => "agent_no_project_dir",
            Side::Global => "agent_no_global_dir",
        }
    }
}

mod dest;
mod dry;
mod list;
mod spec;
mod unuse;
mod unuse_agent;
mod unuse_global;
mod upgrade;
mod upgrade_all;
mod upgrade_gates;

/// `use` 是关键字，使用流程测试住在 use.rs
#[path = "use.rs"]
mod use_flow;
