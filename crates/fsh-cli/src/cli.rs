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
    /// 初始化 hub（幂等：已初始化只报告，内容一致则不动）
    Init {
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 入库一个本地 skill 目录（复制 + 自动提交，一步到位）
    Add {
        /// skill 源目录（须含带 frontmatter 的 SKILL.md）
        dir: PathBuf,
        /// 键的组织前缀（如 mattpocock/skills），缺省入根下
        #[arg(long)]
        prefix: Option<String>,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 浏览库内技能：一行一个（键、description、使用清单、source）
    #[command(visible_alias = "ls")]
    List {
        /// 过滤词：键 / description 子串匹配
        filter: Option<String>,
        /// 只列归档技能（缺省只列未归档）
        #[arg(long)]
        archived: bool,
    },
    /// 归档 skill（移出使用面，历史留审计日志）
    Archive {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 彻底删除 skill（被项目引用时拒绝）
    #[command(visible_alias = "rm")]
    Remove {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 把归档的 skill 移回 store（store 同名键占用报冲突）
    Restore {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 重组技能键前缀（挪 store 目录 + 改写登记 + 重链落点；末段不可变）
    #[command(visible_alias = "mv")]
    Move {
        /// 旧键（store 内相对路径，`/` 分段）
        from: String,
        /// 新键（末段必须与旧键一致，前缀随意重组）
        to: String,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 首次把 skill 铺给全局技能目录或某几个项目的 agent（--agent 必填；在场 + 无登记 + 落点全空闲）
    Use {
        /// 技能键（store 内相对路径，`/` 分段）
        skill: String,
        /// 要铺给的 agent（逗号分隔、可重复）
        #[arg(short = 'a', long, value_delimiter = ',', required = true)]
        agent: Vec<String>,
        /// 服务全局技能目录（可与 --project 并用）
        #[arg(short = 'g', long)]
        global: bool,
        /// 项目根目录（须已存在），可重复；要用当前项目请显式写 --project .
        #[arg(short = 'p', long)]
        project: Vec<PathBuf>,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 撤回技能：撤走 --agent 名单里的 agent，名单点满即整条记录撤回
    Unuse {
        skill: String,
        /// 要撤的 agent（逗号分隔、可重复）
        #[arg(short = 'a', long, value_delimiter = ',', required = true)]
        agent: Vec<String>,
        /// 撤回全局技能目录里的落点（可与 --project 并用）
        #[arg(short = 'g', long)]
        global: bool,
        /// 项目根目录（须已存在），可重复；要用当前项目请显式写 --project .
        #[arg(short = 'p', long)]
        project: Vec<PathBuf>,
        /// 覆盖「copy 内容被修改」的安全闸
        #[arg(short = 'f', long)]
        force: bool,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
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
    /// 环境体检（有 FAIL 项时退出码 1）
    Doctor,
}
