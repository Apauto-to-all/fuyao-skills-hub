//! 内置表条目的全局根适配声明：专有环境变量、XDG 配置根、多候选目录
//! 存在性探测与平台分叉回落。解析只产出根目录下的技能子路径；变量未设置
//! 或取值不可用（空值、相对路径）时静默回落表里的默认字面量，不报错。
//! 家目录、变量取值与平台标志由调用方注入，本模块测试不碰真实环境。

use std::path::{Path, PathBuf};

/// 一条内置条目的全局根解析方式；`sub` 是根目录下的技能子路径
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobalRoot {
    /// 专有变量根：变量设置且取值为绝对路径时以变量值为根，否则回落表字面量
    Var {
        /// 环境变量名
        name: &'static str,
        /// 根目录下的技能子路径
        sub: &'static str,
    },
    /// 多候选目录存在性探测：按序取第一个存在的候选目录，
    /// 全不存在用第一个候选
    Probe {
        /// 候选目录名（家目录下的相对段），按优先级排序
        candidates: &'static [&'static str],
        /// 候选目录下的技能子路径
        sub: &'static str,
    },
    /// 变量根加 Windows 目录回落：变量优先；Windows 上未设置时回落
    /// `APPDATA`（未设置再回落家目录 Roaming 路径）下的产品目录，
    /// 非 Windows 回落表字面量
    VarWin {
        /// 环境变量名
        name: &'static str,
        /// Windows 回落时拼接的产品目录名（APPDATA 与家目录 Roaming 同名）
        win_dir: &'static str,
        /// 根目录下的技能子路径
        sub: &'static str,
    },
}

/// 解析一个带适配声明的全局技能目录；失败原因是给人读的一句话。
/// `raw` 是表里的默认字面量，回落时按可移植写法展开它
pub(super) fn resolve(
    root: GlobalRoot,
    raw: &str,
    home: &Path,
    windows: bool,
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, String> {
    match root {
        GlobalRoot::Var { name, sub } => match absolute_value(name, lookup) {
            Some(dir) => Ok(dir.join(sub)),
            None => fallback(raw, home, lookup),
        },
        GlobalRoot::Probe { candidates, sub } => {
            // 全不存在时取第一个候选，与回落字面量同义
            let dir = candidates
                .iter()
                .find(|c| home.join(c).exists())
                .copied()
                .unwrap_or(candidates[0]);
            Ok(home.join(dir).join(sub))
        }
        GlobalRoot::VarWin { name, win_dir, sub } => match absolute_value(name, lookup) {
            Some(dir) => Ok(dir.join(sub)),
            None if windows => match absolute_value("APPDATA", lookup) {
                Some(appdata) => Ok(appdata.join(win_dir).join(sub)),
                None => Ok(home.join("AppData/Roaming").join(win_dir).join(sub)),
            },
            None => fallback(raw, home, lookup),
        },
    }
}

/// 取一个变量的绝对路径值：未设置、空值或相对路径都返回 None。
/// 落点必须是绝对路径，取值不可用时宁可回落也不产出相对落点
fn absolute_value(name: &str, lookup: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    lookup(name)
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .filter(|dir: &PathBuf| dir.is_absolute())
}

/// 回落路径：按可移植写法展开表里的默认字面量
fn fallback(
    raw: &str,
    home: &Path,
    lookup: &impl Fn(&str) -> Option<String>,
) -> Result<PathBuf, String> {
    super::portable_dir::expand(raw, home, lookup)
}
