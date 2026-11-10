//! 自检：九项健康检查，只报不动手——诊断命令永远不可能弄坏数据。
//! 输出结构化报告（检查项 + ok/warn/fail + 说明），GUI 与 CLI 直接渲染。

use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::builtin;
use crate::error::{CommonError, common_error_from};
use crate::paths::HubPaths;

mod checks;

/// 自检错误（单条检查失败不算：那是 fail 状态的检查项，不是错误）
#[derive(Debug, Error)]
pub enum DoctorError {
    /// 底层失败（文件系统）
    #[error(transparent)]
    Common(#[from] CommonError),
}

common_error_from!(DoctorError: std::io::Error);

impl ErrorCode for DoctorError {
    fn error_code(&self) -> &'static str {
        match self {
            DoctorError::Common(c) => c.error_code(),
        }
    }
}

/// 单条检查结论
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DoctorCheck {
    /// 检查项名（稳定标识，供 --json 消费方匹配）
    pub name: String,
    /// 结论
    pub status: CheckStatus,
    /// 人读说明
    pub detail: String,
}

/// 检查结论三态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// 通过
    Ok,
    /// 可疑但可用（有兜底或暂不致命）
    Warn,
    /// 不满足运行前提
    Fail,
}

/// 自检报告
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct DoctorReport {
    /// 全部检查项
    pub checks: Vec<DoctorCheck>,
}

/// 运行九项自检：hub 根可写 / 非云同步路径 /
/// 链接能力实测三级 / hub 装配与可读性 / 内置技能铺到全局 / 内置技能内容对齐 /
/// 登记簿引用但导出目录缺失 / 登记簿项目路径不存在 / 可回收超阈值。只报不动手。
pub fn doctor(hub_root: &Path) -> Result<DoctorReport, DoctorError> {
    // 耗时起点：全量检查跨写探针、建链接、gc dry-run 多次外部调用
    let started = Instant::now();
    let paths = HubPaths::from_root(hub_root);
    // 内嵌表与登记簿各只取一次：下面依赖它们的检查共用同一份读取结果
    let builtin_skills = builtin::skills();
    let registry = checks::read_registry(&paths);
    let mut checks = Vec::with_capacity(9);
    checks.push(checks::hub_writable(&paths));
    checks.push(checks::cloud_path(paths.root()));
    checks.push(checks::link_capability());
    checks.push(checks::hub_integrity(&paths));
    checks.push(checks::builtin_global(&builtin_skills, &registry));
    checks.push(checks::builtin_content(&paths, &builtin_skills));
    checks.extend(checks::registry(&paths, &registry));
    checks.push(checks::reclaimable(&paths));
    // 全量检查完成是里程碑：三态计数与耗时让健康度与慢项一眼可见
    let tally = |status: CheckStatus| checks.iter().filter(|c| c.status == status).count();
    tracing::info!(
        path = %paths.root().display(),
        count = checks.len(),
        ok = tally(CheckStatus::Ok),
        warn = tally(CheckStatus::Warn),
        fail = tally(CheckStatus::Fail),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "doctor 检查完成"
    );
    Ok(DoctorReport { checks })
}

#[cfg(test)]
mod tests;
