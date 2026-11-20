//! 输出塑形：命令名登记、成功 / 失败信封与结果载荷分派。--json 时 stdout
//! 一行统一信封 `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 文本模式逐命令给人读形态：预演列动作清单、写命令一句完成陈述、list 分块、
//! search 按视图展开、doctor 逐项结论。机器格式只有 --json 一条路。
//! 信封键名与形状只在各出口构造器一处声明。

mod search_text;

pub(crate) use search_text::{SearchView, render_search};

#[cfg(test)]
mod tests;

use std::fmt::{self, Display};
use std::path::Path;

use fsh_core::ErrorCode;
use fsh_core::Verb;
use fsh_core::doctor::{CheckStatus, DoctorReport};
use fsh_core::hub::{InitOutcome, Scope, SkillUsage};
use fsh_core::link::LinkKind;
use fsh_core::plan::{ActionKind, PlanStep};
use fsh_core::search::SearchResult;
use fsh_core::skill::{MoveOutcome, SelectionOutcome, SkillRow, Skipped};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::cli::Cmd;

/// 子命令的固定名字：日志里要稳定可查，不用 Debug（会带出参数值与类型括号）
pub(crate) fn cmd_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Init { .. } => "init",
        Cmd::Add { .. } => "add",
        Cmd::List { .. } => "list",
        Cmd::Archive { .. } => "archive",
        Cmd::Remove { .. } => "remove",
        Cmd::Restore { .. } => "restore",
        Cmd::Move { .. } => "move",
        Cmd::Use { .. } => "use",
        Cmd::Unuse { .. } => "unuse",
        Cmd::Search { .. } => "search",
        Cmd::Doctor => "doctor",
    }
}

/// 失败信封载荷：code 取 core 错误的稳定码（ErrorCode 契约，显式声明、
/// 包装变体透传底层真因），message 取 Display 文本
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

/// 任意 core 错误统一收敛为失败信封
pub(crate) fn fail(e: impl Display + ErrorCode) -> Failure {
    Failure {
        code: e.error_code(),
        message: e.to_string(),
    }
}

/// CLI 自己的失败：参数层挡下的用法越界，与 core 错误走同一个失败信封
pub(crate) enum CliError {
    /// 参数组合越界（与 core 无关，如上下文 flag 与 --json 同给）
    Usage(String),
}

impl Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Usage(message) => f.write_str(message),
        }
    }
}

impl ErrorCode for CliError {
    fn error_code(&self) -> &'static str {
        match self {
            CliError::Usage(_) => "usage",
        }
    }
}

/// 命令执行结果：载荷形态自己说明是机器信封还是人读文本，
/// 退出码随命令语义（doctor 出 FAIL 为 1）
pub(crate) struct Outcome {
    /// 进程退出码
    pub(crate) code: i32,
    /// 结果载荷
    pub(crate) body: Body,
}

/// 结果载荷：一条命令只产生其中一种——机器信封的 data 节点，或人读的逐行文本
pub(crate) enum Body {
    /// --json 信封的 data 节点
    Data(Value),
    /// 文本模式的逐行输出
    Text(Vec<String>),
}

/// 序列化辅助：信封载荷的序列化无失败路径，失败兜底 null 不炸 CLI
fn to_json<T: Serialize + ?Sized>(v: &T) -> Value {
    // 载荷全是本项目的 Serialize 结构（无 NaN、无非法 map 键），序列化不会失败；
    // 兜底只为让出口不 panic——真落到 null 说明载荷形状变了，值得查
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// 写命令的载荷：--json 取载荷本体（预演与实跑同一形状）；
/// 文本模式预演列动作清单，实跑给人读的结果行
fn write_body<T: Serialize>(
    json: bool,
    r: &T,
    dry_run: bool,
    steps: Vec<PlanStep>,
    lines: Vec<String>,
) -> Body {
    if json {
        Body::Data(to_json(r))
    } else if dry_run {
        Body::Text(step_lines(steps))
    } else {
        Body::Text(lines)
    }
}

/// 预演步骤的文本形态：逐行 `- <步骤>`；无可执行步骤时给一句结论，不留空输出
fn step_lines(steps: Vec<PlanStep>) -> Vec<String> {
    if steps.is_empty() {
        return vec!["预演：无待执行步骤".to_string()];
    }
    steps
        .iter()
        .map(|step| format!("- {}", step_line(step)))
        .collect()
}

/// 缩进一行的字段说明：`  标签：值`
fn note_line(label: &str, value: &str) -> String {
    format!("  {label}：{value}")
}

/// init 的出口：预演列装配步骤；实跑按首次装配、补齐缺件、幂等重跑三种结果报一句。
/// `root` 是 hub 根，结果行里写明装在哪里
pub(crate) fn init_outcome(r: &InitOutcome, root: &Path, json: bool) -> Outcome {
    let head = match (r.initialized, r.repaired.is_empty()) {
        (true, true) => "技能库初始化完成",
        (true, false) => "技能库修复完成",
        (false, _) => "技能库已初始化，本次未做改动",
    };
    let mut lines = vec![
        head.to_string(),
        note_line("位置", &root.display().to_string()),
    ];
    if !r.repaired.is_empty() {
        lines.push(note_line("补齐", &r.repaired.join("、")));
    }
    let body = write_body(json, r, r.dry_run, r.actions.clone(), lines);
    Outcome { code: 0, body }
}

/// 完成陈述的动词措辞：四个写动词各一句，措辞只在本模块定
fn done_word(verb: Option<Verb>) -> &'static str {
    match verb {
        Some(Verb::Add) => "入库完成",
        Some(Verb::Archive) => "归档完成",
        Some(Verb::Remove) => "移除完成",
        Some(Verb::Restore) => "恢复完成",
        Some(Verb::Use) => "使用完成",
        // 实跑必带动词；预演走动作清单，读不到这里
        _ => "完成",
    }
}

/// 选择器写命令的出口：预演列动作与跳过项，实跑一句完成陈述 + 跳过清单。
/// 退出码取载荷本身：一条都没做成、却有被跳过的项 → 1
pub(crate) fn selection_outcome(r: &SelectionOutcome, json: bool) -> Outcome {
    selection_body(r, json, done_word(r.verb))
}

/// 撤回的出口：与选择器写命令同形，完成陈述用项目面的措辞——撤回不碰 hub 的
/// 跟踪面、不产生机器提交，因此没有动词可取名。退出码同 selection_outcome
pub(crate) fn unuse_outcome(r: &SelectionOutcome, json: bool) -> Outcome {
    selection_body(r, json, "解除使用完成")
}

/// 选择器载荷的出口：--json 直出载荷本体，文本模式预演列整份计划、
/// 实跑给完成陈述；跳过项两种形态都逐条列出
fn selection_body(r: &SelectionOutcome, json: bool, done: &str) -> Outcome {
    let body = if json {
        Body::Data(to_json(r))
    } else {
        Body::Text(selection_lines(r, done))
    };
    Outcome {
        code: r.code(),
        body,
    }
}

/// 文本形态：预演列整份计划，实跑是完成陈述；跳过项两种都逐条列出
fn selection_lines(r: &SelectionOutcome, done: &str) -> Vec<String> {
    let mut lines = if r.dry_run {
        step_lines(r.actions.clone())
    } else {
        done_lines(r, done)
    };
    lines.extend(r.skipped.iter().map(skip_line));
    lines
}

/// 实跑的完成陈述：单键一句带键，整批先报条数再逐条列出做掉的键；
/// 有跳过项时在首行补一笔，人读一眼看到整批没全做
fn done_lines(r: &SelectionOutcome, done: &str) -> Vec<String> {
    let mut lines = match r.applied.as_slice() {
        [] => vec![format!("{done}：无")],
        [key] => vec![format!("{done}：{key}")],
        keys => {
            let mut lines = vec![format!("{done}：{} 个技能", keys.len())];
            lines.extend(keys.iter().map(|key| format!("- {key}")));
            lines
        }
    };
    if !r.skipped.is_empty() {
        let tail = format!("，跳过 {} 个", r.skipped.len());
        lines[0].push_str(&tail);
    }
    lines
}

/// 跳过项一行：键 + 稳定码 + 原因
fn skip_line(s: &Skipped) -> String {
    format!("- 跳过 {}（{}）: {}", s.key, s.code, s.message)
}

/// move 的出口：预演列步骤；实跑报两端键，登记改写数与重链落点数在非零时补出
pub(crate) fn move_outcome(r: &MoveOutcome, json: bool) -> Outcome {
    let mut lines = vec![format!("移动完成：{} -> {}", r.from, r.to)];
    if r.registry_edits > 0 {
        lines.push(note_line("登记改写", &format!("{} 条", r.registry_edits)));
    }
    if !r.relinked.is_empty() {
        lines.push(note_line("重链落点", &format!("{} 处", r.relinked.len())));
    }
    let body = write_body(json, r, r.dry_run, r.actions.clone(), lines);
    Outcome { code: 0, body }
}

/// search 的出口：--json 出检索信封（hits/total/truncated）；文本模式按视图塑形
pub(crate) fn search_outcome(res: &SearchResult, view: SearchView, json: bool) -> Outcome {
    let body = if json {
        Body::Data(to_json(res))
    } else {
        Body::Text(render_search(res, view))
    };
    Outcome { code: 0, body }
}

/// 文本模式 description 的显示宽度上限
const DESC_TRUNC_CHARS: usize = 60;
/// 文本模式字段标签的显示宽度：Description / Uses 两种标签左对齐到同一列
const LABEL_WIDTH: usize = 12;
/// 文本模式使用清单的列间隔：目标列与链接列各按块内最长值补空格
const USES_COLUMN_GAP: usize = 3;

/// list 的出口：`{"skills": 行集}` 信封；文本模式按面分块——面标题与空行之后，
/// 一块一个技能（块间空行）：键原样、Description / Uses 字段行。
/// description 显示层截断，JSON 与 data 保持全文；uses 为空时整行不出。
/// 使用清单一条登记记录一行：目标、链接方式、agent 名单
pub(crate) fn list_outcome(rows: &[SkillRow], archived: bool, json: bool) -> Outcome {
    let title = if archived {
        "Archived skills"
    } else {
        "Store skills"
    };
    let mut lines = vec![title.to_string(), String::new()];
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            lines.push(String::new());
        }
        lines.push(row.key.clone());
        let desc: String = row.description.chars().take(DESC_TRUNC_CHARS).collect();
        lines.push(field_line("Description:", &desc));
        if !row.uses.is_empty() {
            lines.extend(uses_lines(&row.uses));
        }
    }
    let data = json!({"skills": rows});
    let body = if json {
        Body::Data(data)
    } else {
        Body::Text(lines)
    };
    Outcome { code: 0, body }
}

/// 字段行：标签左对齐到 LABEL_WIDTH，值接在同一列
fn field_line(label: &str, value: &str) -> String {
    format!("  {label:<LABEL_WIDTH$} {value}")
}

/// 使用清单行：一条登记记录一行，首行带 Uses 标签，后续行用等宽空白接着列。
/// 目标列与链接列按本块内的最长值补空格对齐，只补空格不改内容
fn uses_lines(uses: &[SkillUsage]) -> Vec<String> {
    let targets: Vec<String> = uses
        .iter()
        .map(|u| match u.scope {
            Scope::Project => u.project.clone().unwrap_or_else(|| "project".to_string()),
            Scope::Global => "global".to_string(),
        })
        .collect();
    let links: Vec<&str> = uses.iter().map(|u| link_name(u.link)).collect();
    // 宽度按字符数算：格式串的填充宽度也以字符计
    let target_width = targets.iter().map(|t| t.chars().count()).max().unwrap_or(0);
    let link_width = links.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let gap = " ".repeat(USES_COLUMN_GAP);
    let indent = " ".repeat(2 + LABEL_WIDTH + 1);
    uses.iter()
        .enumerate()
        .map(|(i, usage)| {
            let row = format!(
                "{:<target_width$}{gap}{:<link_width$}{gap}{}",
                targets[i],
                links[i],
                usage.agents.join(", ")
            );
            if i == 0 {
                format!("  {:<LABEL_WIDTH$} {row}", "Uses:")
            } else {
                format!("{indent}{row}")
            }
        })
        .collect()
}

/// 链接方式的文本名：与登记簿里的取值同名，人读与机器读对得上
fn link_name(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Symlink => "symlink",
        LinkKind::Junction => "junction",
        LinkKind::Copy => "copy",
    }
}

/// doctor 的出口：`{"checks": 行集, "fail": 有无}` 信封；查出 FAIL 退出码 1。
/// 文本模式逐项一行「结论 + 检查项 + 说明」，末尾一行合计
pub(crate) fn doctor_outcome(r: &DoctorReport, json: bool) -> Outcome {
    let has_fail = r.checks.iter().any(|c| c.status == CheckStatus::Fail);
    let mut fields = Map::new();
    fields.insert("checks".to_string(), to_json(&r.checks));
    fields.insert("fail".to_string(), json!(has_fail));
    let name_width = r
        .checks
        .iter()
        .map(|c| c.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut lines: Vec<String> = r
        .checks
        .iter()
        .map(|c| {
            format!(
                "{}  {:<name_width$}  {}",
                status_name(c.status),
                c.name,
                c.detail
            )
        })
        .collect();
    let count = |status: CheckStatus| r.checks.iter().filter(|c| c.status == status).count();
    lines.push(String::new());
    lines.push(format!(
        "共 {} 项：{} 项通过，{} 项可疑，{} 项失败",
        r.checks.len(),
        count(CheckStatus::Ok),
        count(CheckStatus::Warn),
        count(CheckStatus::Fail)
    ));
    let body = if json {
        Body::Data(Value::Object(fields))
    } else {
        Body::Text(lines)
    };
    Outcome {
        code: i32::from(has_fail),
        body,
    }
}

/// 检查结论的文本名：三态各一个词，与信封里的 ok / warn / fail 一一对应
fn status_name(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "通过",
        CheckStatus::Warn => "可疑",
        CheckStatus::Fail => "失败",
    }
}

/// 预演步骤的文本形态：动作词 + 路径 + 对象，一行一件事，不加引号。
/// 形状由字段组合决定：有来源与落点写箭头（复制、移动、建落点）；
/// 只有对象写对象（写盘、删除、装配）；对象加落点写「<对象> in <落点>」（登记）；
/// 提交没有路径，消息即对象，冒号后整段给出。
/// 带 agent 名单的步骤在行尾补一段括号，人读时看得出这一步为谁做
fn step_line(step: &PlanStep) -> String {
    let action = step.action.as_str();
    let mut line = match (&step.from, &step.to, &step.target) {
        (_, _, Some(target)) if step.action == ActionKind::Commit => {
            format!("{action}: {target}")
        }
        (Some(from), Some(to), _) => format!("{action} {from} -> {to}"),
        (_, Some(to), Some(target)) => format!("{action} {target} in {to}"),
        (_, _, Some(target)) => format!("{action} {target}"),
        (_, Some(to), None) => format!("{action} {to}"),
        // 六个写入口的步骤恒有来源或对象，空步骤是构造错误，宁可留白也不 panic
        _ => action.to_string(),
    };
    if !step.agents.is_empty() {
        line.push_str(&format!("（agents: {}）", step.agents.join(", ")));
    }
    line
}

/// 成功输出：--json 一行信封；文本模式逐行平铺结果行
pub(crate) fn emit_ok(oc: &Outcome) {
    match &oc.body {
        Body::Data(data) => println!("{}", json!({"ok": true, "data": data})),
        Body::Text(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
    }
}

/// 失败输出：--json 一行信封；文本模式进 stderr（不污染 stdout 数据流）
pub(crate) fn emit_err(json: bool, f: &Failure) {
    if json {
        println!(
            "{}",
            json!({"ok": false, "error": {"code": f.code, "message": f.message}})
        );
        return;
    }
    eprintln!("错误 [{}]: {}", f.code, f.message);
}
