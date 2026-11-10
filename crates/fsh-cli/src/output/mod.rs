//! 输出塑形：命令名登记、成功 / 失败信封与文本形态分派。--json 时 stdout
//! 一行统一信封 `{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`；
//! 文本模式按命令在编译期决定的形态渲染：动作清单逐行、diff 逐文件
//! unified patch 原文（人读优先）、search 平铺、其余 pretty JSON。
//! 信封键名与形状只在各出口构造器一处声明。

mod search_text;

pub(crate) use search_text::render_search;

#[cfg(test)]
mod tests;

use std::fmt::Display;

use fsh_core::ErrorCode;
use fsh_core::agents::AgentInfo;
use fsh_core::doctor::{CheckStatus, DoctorReport};
use fsh_core::gc::GcReport;
use fsh_core::hub::InitOutcome;
use fsh_core::project::ProjectView;
use fsh_core::skill::{
    AddOutcome, ArchiveOutcome, DiffFile, ReleaseOutcome, RemoveOutcome, RestoreOutcome,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::cli::Cmd;

/// 子命令的固定名字：日志里要稳定可查，不用 Debug（会带出参数值与类型括号）
pub(crate) fn cmd_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Init { .. } => "init",
        Cmd::Add { .. } => "add",
        Cmd::Show { .. } => "show",
        Cmd::Release { .. } => "release",
        Cmd::Archive { .. } => "archive",
        Cmd::Remove { .. } => "remove",
        Cmd::Restore { .. } => "restore",
        Cmd::Agents => "agents",
        Cmd::Projects => "projects",
        Cmd::Use { .. } => "use",
        Cmd::Unuse { .. } => "unuse",
        Cmd::Upgrade { .. } => "upgrade",
        Cmd::Diff { .. } => "diff",
        Cmd::Search { .. } => "search",
        Cmd::Gc { .. } => "gc",
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
/// 退出码随命令语义（doctor 出 FAIL、批量升级中途失败为 1）
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
    /// 动作清单逐行 `- <JSON 值>`（预演的字符串项与 gc 的动作对象同规则）
    Actions(Vec<Value>),
    /// diff 逐文件输出 unified patch 原文
    Diff(Vec<DiffFile>),
    /// 逐行平铺（search 的 -l/-c/上下文塑形结果）
    Lines(Vec<String>),
    /// 其余载荷：pretty JSON
    Json(Value),
}

/// 预演动作清单的载荷：顶层 `actions` 仅预演出现，非空即文本模式逐行列出。
/// use / unuse / upgrade 的动作清单在逐目标载荷里，文本模式与实跑同走
/// pretty JSON，不属此列
pub(crate) trait ActionOutcome {
    /// 将执行的动作清单；实跑载荷恒为空
    fn actions(&self) -> &[String];
}

/// 六个写命令载荷同形：顶层 actions 直接透出
macro_rules! action_outcomes {
    ($($t:ty),* $(,)?) => {
        $(
            impl ActionOutcome for $t {
                fn actions(&self) -> &[String] {
                    &self.actions
                }
            }
        )*
    };
}

action_outcomes!(
    InitOutcome,
    AddOutcome,
    ReleaseOutcome,
    ArchiveOutcome,
    RemoveOutcome,
    RestoreOutcome,
);

/// 序列化辅助：信封载荷的序列化无失败路径，失败兜底 null 不炸 CLI
fn to_json<T: Serialize + ?Sized>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// 写命令的通用出口：data 直出载荷本体；预演（动作清单非空）文本逐行
/// 列动作，实跑 pretty JSON
pub(crate) fn plain<T: Serialize + ActionOutcome>(r: &T, code: i32) -> Outcome {
    let data = to_json(r);
    let text = if r.actions().is_empty() {
        TextForm::Json(data.clone())
    } else {
        TextForm::Actions(r.actions().iter().map(|a| json!(a)).collect())
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

/// 单键信封：`{<键>: 值}`；文本模式 pretty JSON
fn envelope(key: &str, value: Value) -> Outcome {
    let mut data = Map::new();
    data.insert(key.to_string(), value);
    let data = Value::Object(data);
    Outcome {
        text: TextForm::Json(data.clone()),
        data,
        code: 0,
    }
}

/// agents 的出口：`{"agents": 行集}` 信封
pub(crate) fn agents_outcome(rows: &[AgentInfo]) -> Outcome {
    envelope("agents", to_json(rows))
}

/// projects 的出口：`{"projects": 行集}` 信封
pub(crate) fn projects_outcome(rows: &[ProjectView]) -> Outcome {
    envelope("projects", to_json(rows))
}

/// diff 的出口：`{"files": 行集}` 信封，文本逐文件输出 unified patch 原文
pub(crate) fn diff_outcome(files: Vec<DiffFile>) -> Outcome {
    let data = json!({"files": &files});
    Outcome {
        text: TextForm::Diff(files),
        data,
        code: 0,
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

/// gc 的出口：`{"actions": 动作清单, "freed_bytes": 字节}` 信封；文本模式
/// 动作逐行（含实跑——清单记录已发生的事，空清单零输出）
pub(crate) fn gc_outcome(r: &GcReport) -> Outcome {
    let mut fields = Map::new();
    fields.insert("actions".to_string(), to_json(&r.items));
    fields.insert("freed_bytes".to_string(), json!(r.freed_bytes));
    Outcome {
        text: TextForm::Actions(r.items.iter().map(to_json).collect()),
        data: Value::Object(fields),
        code: 0,
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

/// 成功输出：--json 一行信封；文本模式按形态渲染——动作清单逐行、
/// diff 逐文件输出 unified patch 原文（人读优先）、search 平铺、
/// 其余 pretty JSON
pub(crate) fn emit_ok(json: bool, oc: &Outcome) {
    if json {
        println!("{}", json!({"ok": true, "data": oc.data}));
        return;
    }
    match &oc.text {
        TextForm::Actions(items) => {
            for a in items {
                println!("- {a}");
            }
        }
        // patch 字段本身就是 unified diff 原文，直接打印
        TextForm::Diff(files) => {
            for f in files {
                println!("diff {}", f.path);
                print!("{}", f.patch);
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
