//! 预演步骤：写命令「将会做什么」的结构化描述。
//! 一步只描述一件事：做什么（[`ActionKind`]）、从哪到哪（`from` / `to`）、
//! 对象（`target`）与涉及的 agent 名单。措辞与引号不属于本模块——文本形态由
//! 出口层按这些字段拼，core 只给事实。
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

use serde::{Deserialize, Serialize};

/// 写计划：一次写操作将要做的全部步骤，按执行顺序。
/// 计划是步骤的唯一来源：预演清单从它投影，机器提交按它的提交步逐条落。
/// 每条命令自己排步骤，写骨架只认这个面
pub(crate) trait Steps {
    /// 本计划的全部步骤
    fn steps(&self) -> Vec<PlanStep>;
}

/// 步骤清单本身也是一种计划：计划形状就是一张平铺清单时直接用它
impl Steps for Vec<PlanStep> {
    fn steps(&self) -> Vec<PlanStep> {
        self.clone()
    }
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
