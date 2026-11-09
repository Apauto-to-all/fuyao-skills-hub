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
        /// 键的组织前缀（如 mattpocock/skills），缺省入根下
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
    /// 首次把 skill 铺给全局技能目录或某几个项目的 agent（未归档 + 无登记 + 落点全空闲）
    Use {
        /// 技能说明：`<skill>` 跟随最新，`<skill>@<版本>` 用固定版本
        skill: String,
        /// 目标 agent，逗号分隔
        #[arg(long, value_delimiter = ',')]
        agent: Vec<String>,
        /// 服务全局技能目录（可与 --project 并用）
        #[arg(long)]
        global: bool,
        /// 项目根目录（须已存在），可重复；要用当前项目请显式写 --project .
        #[arg(long)]
        project: Vec<PathBuf>,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 撤回技能：给 --agent 按 agent 撤，不给则整体撤回
    Unuse {
        skill: String,
        /// 只撤这些 agent 的落点与登记（逗号分隔、可重复）；不给则整体撤回
        #[arg(long, value_delimiter = ',')]
        agent: Option<Vec<String>>,
        /// 撤回全局技能目录里的落点（可与 --project 并用）
        #[arg(long)]
        global: bool,
        /// 项目根目录（须已存在），可重复；要用当前项目请显式写 --project .
        #[arg(long)]
        project: Vec<PathBuf>,
        /// 覆盖「copy 内容被修改」的安全闸
        #[arg(long)]
        force: bool,
        /// 只列将执行的动作，不落盘
        #[arg(long)]
        dry_run: bool,
    },
    /// 把已登记的技能切换到指定版本或跟随最新（旧入口先占位、失败整批还原）；
    /// `--all` 则把范围内全部落后记录逐条升到各自最新版本
    Upgrade {
        /// 技能说明：`<skill>` 跟随最新，`<skill>@<版本>` 切到该版本（往旧切即回滚）；
        /// 与 --all 二选一
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        skill: Option<String>,
        /// 一次升级范围内全部「固定版本且落后」的记录，逐条升到该技能最新版本
        #[arg(long)]
        all: bool,
        /// 服务全局技能目录（可与 --project 并用）；--all 时只把范围收窄到全局
        #[arg(long)]
        global: bool,
        /// 项目根目录（须已存在），可重复；要用当前项目请显式写 --project .
        /// --all 时只把范围收窄到这些项目，不给才是全 hub
        #[arg(long)]
        project: Vec<PathBuf>,
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
    /// 检索 hub 内文件内容（文本行匹配；smart-case 正则缺省）
    Search {
        /// 检索模式：缺省按正则解释（smart-case），-F 后按字面量
        pattern: String,
        /// 相对 hub 根的检索范围（如 store/<键>、agents）；缺省整库
        path: Option<String>,
        /// 强制忽略大小写（缺省 smart-case：模式全小写才忽略）
        #[arg(short = 'i')]
        case_insensitive: bool,
        /// pattern 按字面量解释（元字符不再当正则）
        #[arg(short = 'F')]
        literal: bool,
        /// 整词匹配（词边界锚定）
        #[arg(short = 'w')]
        whole_word: bool,
        /// glob 收窄文件面（可重复，! 开头剔除）；只能收窄，放不回被忽略的路径
        #[arg(short = 'g')]
        glob: Vec<String>,
        /// 只列命中文件
        #[arg(short = 'l', conflicts_with = "count")]
        files_with_matches: bool,
        /// 每文件命中计数
        #[arg(short = 'c')]
        count: bool,
        /// 命中行后附 N 行上下文（仅文本模式；与 --json 同给报用法冲突）
        #[arg(short = 'A', value_name = "N", conflicts_with_all = ["before", "context"])]
        after: Option<usize>,
        /// 命中行前附 N 行上下文（仅文本模式；与 --json 同给报用法冲突）
        #[arg(short = 'B', value_name = "N", conflicts_with_all = ["after", "context"])]
        before: Option<usize>,
        /// 命中行前后各附 N 行上下文（仅文本模式；与 --json 同给报用法冲突）
        #[arg(short = 'C', value_name = "N", conflicts_with_all = ["after", "before"])]
        context: Option<usize>,
        /// 命中行数上限（缺省 200；截断只裁 hits，total 仍精确）
        #[arg(long)]
        limit: Option<usize>,
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
