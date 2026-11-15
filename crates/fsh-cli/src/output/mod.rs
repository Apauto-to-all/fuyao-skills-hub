//! 输出塑形：命令名登记、成功 / 失败信封与文本形态分派。--json 时 stdout
//! 一行统一信封 `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 文本模式按命令在编译期决定的形态渲染：动作清单逐行、search 平铺、
//! 其余 pretty JSON。
//! 信封键名与形状只在各出口构造器一处声明。

mod search_text;

pub(crate) use search_text::render_search;

#[cfg(test)]
mod tests;

use std::fmt::Display;

use fsh_core::ErrorCode;
use fsh_core::doctor::{CheckStatus, DoctorReport};
use fsh_core::hub::{Scope, SkillUsage};
use fsh_core::link::LinkKind;
use fsh_core::plan::{ActionKind, PlanStep};
use fsh_core::skill::SkillRow;
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

/// 命令执行结果：--json 模式出 data 信封，文本模式按形态渲染，
/// 退出码随命令语义（doctor 出 FAIL 为 1）
pub(crate) struct Outcome {
    /// --json 信封的 data 节点
    pub(crate) data: Value,
    /// 文本模式的渲染形态
    pub(crate) text: TextForm,
    /// 进程退出码
    pub(crate) code: i32,
}

/// 文本模式的渲染形态：由命令分支在编译期指定，与载荷的键名无关
pub(crate) enum TextForm {
    /// 预演步骤逐行 `- <动作与路径>`（措辞只在本模块定）
    Actions(Vec<PlanStep>),
    /// 逐行平铺（search 的 -l/-c/上下文塑形结果）
    Lines(Vec<String>),
    /// 其余载荷：pretty JSON
    Json(Value),
}

/// 序列化辅助：信封载荷的序列化无失败路径，失败兜底 null 不炸 CLI
fn to_json<T: Serialize + ?Sized>(v: &T) -> Value {
    // 载荷全是本项目的 Serialize 结构（无 NaN、无非法 map 键），序列化不会失败；
    // 兜底只为让出口不 panic——真落到 null 说明载荷形状变了，值得查
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// 写命令的通用出口：data 直出载荷本体；预演（动作清单非空）文本逐行
/// 列动作，实跑 pretty JSON
pub(crate) fn plain<T: Serialize + fsh_core::Actions>(r: &T, code: i32) -> Outcome {
    let data = to_json(r);
    let text = if r.actions().is_empty() {
        TextForm::Json(data.clone())
    } else {
        TextForm::Actions(r.actions().to_vec())
    };
    Outcome { data, text, code }
}

/// 只读载荷的出口：data 直出载荷本体，文本模式 pretty JSON
pub(crate) fn query<T: Serialize>(r: &T, code: i32) -> Outcome {
    let data = to_json(r);
    Outcome {
        text: TextForm::Json(data.clone()),
        data,
        code,
    }
}

/// search 文本模式的出口：`{"search_text": 行集}` 信封，文本逐行平铺
pub(crate) fn search_text_outcome(lines: Vec<String>) -> Outcome {
    let data = json!({"search_text": &lines});
    Outcome {
        text: TextForm::Lines(lines),
        data,
        code: 0,
    }
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
pub(crate) fn list_outcome(rows: &[SkillRow], archived: bool) -> Outcome {
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
    Outcome {
        text: TextForm::Lines(lines),
        data,
        code: 0,
    }
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

/// doctor 的出口：`{"checks": 行集, "fail": 有无}` 信封；查出 FAIL 退出码 1
pub(crate) fn doctor_outcome(r: &DoctorReport) -> Outcome {
    let has_fail = r.checks.iter().any(|c| c.status == CheckStatus::Fail);
    let mut fields = Map::new();
    fields.insert("checks".to_string(), to_json(&r.checks));
    fields.insert("fail".to_string(), json!(has_fail));
    Outcome {
        data: Value::Object(fields.clone()),
        text: TextForm::Json(Value::Object(fields)),
        code: i32::from(has_fail),
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

/// 成功输出：--json 一行信封；文本模式按形态渲染——动作清单逐行、
/// search 平铺、其余 pretty JSON
pub(crate) fn emit_ok(json: bool, oc: &Outcome) {
    if json {
        println!("{}", json!({"ok": true, "data": oc.data}));
        return;
    }
    match &oc.text {
        TextForm::Actions(steps) => {
            for step in steps {
                println!("- {}", step_line(step));
            }
        }
        TextForm::Lines(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        TextForm::Json(v) => {
            println!(
                "{}",
                serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".to_string())
            );
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
