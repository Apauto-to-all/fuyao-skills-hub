//! 预演步骤：写命令「将会做什么」的结构化描述。
//! 一步只描述一件事：做什么（[`ActionKind`]）、从哪到哪（`from` / `to`）、
//! 对象（`target`）与涉及的 agent 名单。措辞与引号不属于本模块——文本形态由
//! 出口层按这些字段拼，core 只给事实。
//!
//! 一份计划（[`Plan`]）是带序的步骤值：一步一条，顺序即执行顺序。步骤自带
//! 执行数据（[`Act`]），写骨架按同一次序落盘，预演按同一次序投影描述，机器提交
//! 按其中的提交步逐条落——计划是步骤的唯一来源，动作清单与实跑盘面不会各说一套。
//!
//! serde 形状固定（`--json` 的 `actions` 直接是这些对象的数组）：
//!
//! ```json
//! {"action":"copy","from":"D:/src/demo","to":"store/demo","target":"demo"}
//! {"action":"link","from":"store/demo","to":"D:/proj/.dsh/skills/demo",
//!  "target":"demo","agents":["dsh","universal"]}
//! {"action":"commit","target":"add(store): demo"}
//! ```
//!
//! 规则：`action` 恒出现；`from` / `to` / `target` 缺省即不入信封；
//! `agents` 为空即不入信封。路径一律写完整路径或相对 hub 根的路径，不写引号。
//! `from` 只用于有来源的动作（copy / move / link），写盘与删除的路径写进 `target`；
//! `to` 只用于落点（含 register 的登记位置）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::link::LinkKind;

/// 一份计划：命令自己的业务数据 + 带序的步骤，顺序即执行顺序。
/// 计划是步骤的唯一来源：预演从它投影动作清单，实跑按同一次序落盘，
/// 机器提交按它的提交步逐条落
pub(crate) struct Plan<T> {
    /// 命令自己的业务数据：结果载荷与里程碑取这份，步骤只出盘面
    pub(crate) data: T,
    /// 带序的步骤
    steps: Vec<Step>,
}

impl<T> Plan<T> {
    /// 空计划：业务数据已定，步骤随后按执行顺序追加
    pub(crate) fn new(data: T) -> Self {
        Self {
            data,
            steps: Vec::new(),
        }
    }

    /// 追加一步：描述与执行数据同体
    pub(crate) fn push(&mut self, desc: PlanStep, act: Act) {
        self.steps.push(Step { desc, act });
    }

    /// 全部步骤
    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// 全部步骤的描述：`--json` 的 actions 就是这些对象
    pub(crate) fn describe(&self) -> Vec<PlanStep> {
        self.steps.iter().map(|step| step.desc.clone()).collect()
    }
}

/// 一步计划：预演描述与执行数据同体
pub(crate) struct Step {
    /// 预演描述：出口按这些字段拼文本，`--json` 直出这些对象
    pub(crate) desc: PlanStep,
    /// 执行数据：动盘所需的路径与档位
    pub(crate) act: Act,
}

/// 一步的盘面动作：由写骨架按计划顺序执行，撤销在动手那一刻就地登记
pub(crate) enum Act {
    /// 不动盘：提交、登记这类只有描述的步骤
    None,
    /// 整目录移动：撤销即移回
    Move { from: PathBuf, to: PathBuf },
    /// 复制目录树（跳过 .git）：撤销即删掉落点
    Copy { from: PathBuf, to: PathBuf },
    /// 删目录：不可撤销，失败时盘面留在中途等重跑续完
    Remove(PathBuf),
    /// 建落点：撤销即删掉它。group 是一次使用里目标的序号——同一组的落点
    /// 共用第一个落点定下的档位，登记记录的一个 link 值因此够用
    Link {
        from: PathBuf,
        to: PathBuf,
        group: usize,
    },
    /// 撤落点：按登记的档位删，目标已不在场视为已撤销
    Unlink { path: PathBuf, kind: LinkKind },
    /// 换源重链：撤销即删掉新链、按旧源重建旧链
    Relink {
        path: PathBuf,
        from: PathBuf,
        to: PathBuf,
        kind: LinkKind,
    },
}

/// 一步预演动作的种类：动作词只在这里出现一次，文本形态按它取名
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// 复制目录树（入库）
    Copy,
    /// 整目录移动（归档、恢复）
    Move,
    /// 建落点（项目使用）
    Link,
    /// 删目录（彻底删除、内置技能清孤儿）
    Remove,
    /// 整份写目录（内置技能对齐）
    Write,
    /// 建目录（hub 装配）
    Create,
    /// 初始化 git 仓库
    Init,
    /// 补齐半截装配
    Repair,
    /// 登记簿写一条用法
    Register,
    /// 机器提交：`target` 就是提交消息，写骨架按计划顺序把它逐条提交
    Commit,
}

impl ActionKind {
    /// 动作词：文本模式与 JSON 共用同一套叫法
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Copy => "copy",
            ActionKind::Move => "move",
            ActionKind::Link => "link",
            ActionKind::Remove => "remove",
            ActionKind::Write => "write",
            ActionKind::Create => "create",
            ActionKind::Init => "init",
            ActionKind::Repair => "repair",
            ActionKind::Register => "register",
            ActionKind::Commit => "commit",
        }
    }
}

/// 一步预演动作：做什么、从哪到哪、对象与 agent 名单
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    /// 做什么
    pub action: ActionKind,
    /// 动作的来源（绝对路径或相对 hub 根的路径）；只有 copy / move / link 有来源
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// 动作的落点（绝对路径或相对 hub 根的路径）；无落点的步骤不写
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// 动作对象：写盘与删除的路径、技能键、提交消息这类标识
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// 涉及的 agent 名单；空即不入信封
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
}

impl PlanStep {
    /// 只带动作与对象的一步：提交、装配这类没有来源与落点的动作
    pub fn of(action: ActionKind, target: impl Into<String>) -> Self {
        Self {
            action,
            from: None,
            to: None,
            target: Some(target.into()),
            agents: Vec::new(),
        }
    }

    /// 带来源与落点的一步：复制、移动、建落点这类动盘动作
    pub fn between(action: ActionKind, from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            action,
            from: Some(from.into()),
            to: Some(to.into()),
            target: None,
            agents: Vec::new(),
        }
    }

    /// 补上动作对象：链式写法，读起来就是「这一步对谁做」
    pub fn targeting(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// 补上落点：没有来源的步骤（登记用法这类）只写落点与对象
    pub fn landed_at(mut self, to: impl Into<String>) -> Self {
        self.to = Some(to.into());
        self
    }

    /// 补上 agent 名单
    pub fn with_agents(mut self, agents: Vec<String>) -> Self {
        self.agents = agents;
        self
    }

    /// 机器提交的一步：消息即对象，不写来源与落点。
    /// 提交步必须带消息——落盘段按它提交，空消息会被静默丢弃
    pub fn commit(message: impl Into<String>) -> Self {
        Self::of(ActionKind::Commit, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一份计划：步骤按追加顺序投影
    #[test]
    fn plan_projects_descriptions_in_order() {
        let mut plan = Plan::new(());
        assert!(plan.steps().is_empty(), "新计划没有步骤");
        plan.push(
            PlanStep::between(ActionKind::Move, "store/demo", "archive/demo"),
            Act::Move {
                from: PathBuf::from("store/demo"),
                to: PathBuf::from("archive/demo"),
            },
        );
        plan.push(PlanStep::commit("archive(store): demo"), Act::None);
        assert_eq!(plan.steps().len(), 2, "追加两步就是两步");

        let descs = plan.describe();
        assert_eq!(
            descs.iter().map(|d| d.action).collect::<Vec<_>>(),
            vec![ActionKind::Move, ActionKind::Commit],
            "描述按追加顺序投影"
        );
    }

    /// serde 形状固定：`action` 恒出现，缺省字段不入信封
    #[test]
    fn serde_shape_omits_absent_fields() {
        let step = PlanStep::commit("add(store): demo");
        assert_eq!(
            toml::to_string(&step).unwrap(),
            "action = \"commit\"\ntarget = \"add(store): demo\"\n",
            "无来源与落点的步骤不写这两个字段"
        );

        let step = PlanStep::between(ActionKind::Copy, "D:/src/demo", "store/demo")
            .targeting("demo")
            .with_agents(vec!["dsh".to_string()]);
        assert_eq!(
            toml::to_string(&step).unwrap(),
            concat!(
                "action = \"copy\"\n",
                "from = \"D:/src/demo\"\n",
                "to = \"store/demo\"\n",
                "target = \"demo\"\n",
                "agents = [\"dsh\"]\n",
            )
        );
        // 回读同一形状：字段名与取值都是契约
        let back: PlanStep = toml::from_str(&toml::to_string(&step).unwrap()).unwrap();
        assert_eq!(back, step);
    }

    /// 动作词与枚举一一对应，文本形态照它取名
    #[test]
    fn action_words_cover_every_kind() {
        let all = [
            (ActionKind::Copy, "copy"),
            (ActionKind::Move, "move"),
            (ActionKind::Link, "link"),
            (ActionKind::Remove, "remove"),
            (ActionKind::Write, "write"),
            (ActionKind::Create, "create"),
            (ActionKind::Init, "init"),
            (ActionKind::Repair, "repair"),
            (ActionKind::Register, "register"),
            (ActionKind::Commit, "commit"),
        ];
        for (kind, word) in all {
            assert_eq!(kind.as_str(), word);
            // 动作词同时是 serde 取值：一步动作序列化出来就是这个字符串
            let step = PlanStep::of(kind, "x");
            assert!(
                toml::to_string(&step)
                    .unwrap()
                    .starts_with(&format!("action = \"{word}\"\n")),
                "{word} 的 serde 取值与动作词一致"
            );
        }
    }
}
