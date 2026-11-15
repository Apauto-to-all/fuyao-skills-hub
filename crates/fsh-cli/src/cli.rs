//! 命令行参数定义：全局选项与全部子命令的形状。
//! 只放 clap 结构，不含任何执行逻辑。

use std::path::PathBuf;

use clap::{Command, Parser, Subcommand};

/// 内置 -h / -V 在 build 时才注入，先建出来再改自身说明；分组标题与用法行等固定词汇保持原样
pub(crate) fn localize(mut cmd: Command) -> Command {
    cmd.build();
    let cmd = cmd.mut_args(|arg| match arg.get_id().as_str() {
        "help" => arg.help("显示帮助"),
        "version" => arg.help("显示版本"),
        _ => arg,
    });
    cmd.mut_subcommands(localize)
}

#[derive(Parser)]
#[command(
    name = "fsh",
    version,
    about = "fuyao-skills-hub 技能库命令行",
    disable_help_subcommand = true
)]
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
    /// 初始化技能库
    Init {
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 入库一个技能目录
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
    /// 列出技能
    #[command(visible_alias = "ls")]
    List {
        /// 过滤词：键 / description 子串匹配
        filter: Option<String>,
        /// 只列归档技能（缺省只列未归档）
        #[arg(long)]
        archived: bool,
    },
    /// 归档技能
    Archive {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 删除技能，被引用时拒绝
    #[command(visible_alias = "rm")]
    Remove {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 从归档恢复技能
    Restore {
        skill: String,
        /// 只列将执行的动作，不落盘
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    /// 重组技能键前缀，末段不变
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
    /// 把技能铺给 agent
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
    /// 撤回已铺的技能
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
    /// 检索库内文件内容
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
    /// 环境体检
    Doctor,
}
