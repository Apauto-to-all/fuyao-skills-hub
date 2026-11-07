//! 命令行参数定义：全局选项与全部子命令的形状。
//! 只放 clap 结构，不含任何执行逻辑。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "fsh", version, about = "fuyao-skills-hub 技能库命令行")]
pub(crate) struct Cli {
    /// hub 根目录；优先级 --hub > FSH_HUB > ~/.fuyao-skills-hub
    #[arg(long, global = true)]
    pub(crate) hub: Option<PathBuf>,
    /// 以 JSON 信封输出（stdout 一行）
    #[arg(long, global = true)]
    pub(crate) json: bool,
    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

#[derive(Subcommand)]
pub(crate) enum Cmd {
    /// 初始化 hub（幂等：已初始化只报告，一字节不动）
    Init {
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 入库一个本地 skill 目录（复制 + 首版 tag，一步到位）
    Add {
        /// skill 源目录（须含带 frontmatter 的 SKILL.md）
        dir: PathBuf,
        /// 键的组织前缀（如 anthropics/skills），缺省入根下
        #[arg(long)]
        prefix: Option<String>,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 列出全部 skill（含归档态）
    List {
        /// 按 frontmatter tags 过滤
        #[arg(long)]
        tag: Option<String>,
    },
    /// 查看单个 skill 全量详情（frontmatter / 版本时间线 / 被引用项目）
    Show { skill: String },
    /// 发版：把工作副本当前全部改动（含脏）按 frontmatter 里的号打成新版本
    Release {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 归档 skill（删编辑面与导出目录，版本 tag 与历史全留，可 restore）
    Archive {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 彻底删除 skill（连全部版本 tag 一起删；被项目引用时拒绝）
    Remove {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 从最新版本恢复归档的 skill（复活编辑面，不打新 tag）
    Restore {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 列出认得的 agent（内置 + 用户覆盖）：名字、显示名、目录、配置来源
    Agents,
    /// 列出已登记项目：路径、技能与落点状态
    Projects,
    /// 首次把 skill 铺给某项目的几个 agent（未归档 + 无登记 + 落点全空闲）
    Use {
        /// 技能说明：`<skill>` 跟随最新，`<skill>@<版本>` 用固定版本
        skill: String,
        /// 目标 agent，逗号分隔
        #[arg(long, value_delimiter = ',')]
        agent: Vec<String>,
        /// 项目根目录（须已存在），缺省当前目录
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 整体撤回某项目的某技能（该项目全部 agent 落点）
    Unuse {
        skill: String,
        /// 项目根目录（须已存在），缺省当前目录
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// 覆盖「copy 内容被修改」的安全闸
        #[arg(long)]
        force: bool,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 比较同一 skill 的两个版本（unified diff）
    Diff {
        skill: String,
        v1: String,
        v2: String,
    },
    /// 清理无引用的导出目录（内容与 tag 不符的绝不删）
    Gc {
        /// 只列将清理的项与可释放字节，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 环境体检（有 FAIL 项时退出码 1）
    Doctor,
}
