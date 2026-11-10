//! 错误面共用件：稳定码契约与公共底层错误。
//! 各领域枚举把底层失败透传进公共底层错误：变体形状、Display 文案、
//! 稳定码与 `?` 的 From 路径都只有这里一份。git::GitError 与 hub::HubError
//! 是公共底层错误自身的构件（引用它们会成环），各自保留自己的底层变体声明。

/// 错误稳定码契约：CLI / JSON 消费方靠 code 分支，禁止依赖自由文本。
/// 包装变体委托内层错误码，保证底层真因（如 tag_not_found）直达消费方
/// 而不被外层包装名（如 git）掩盖。
pub trait ErrorCode {
    /// snake_case 稳定码，对外契约的一部分，跨版本保持不变
    fn error_code(&self) -> &'static str;
}

/// 公共底层错误：文件系统、git、文件锁、登记簿与键形状这些在多个领域
/// 重复出现的底层失败，声明、文案与稳定码都收在这里。
/// 领域枚举以 `#[error(transparent)] Common(#[from] CommonError)` 透传本枚举，
/// 于是 Display 与稳定码逐字照底层出，包装层不改写
#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败（未初始化除外，见 HubNotInitialized）
    #[error("git 错误: {0}")]
    Git(crate::git::GitError),
    /// 文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] crate::lock::LockError),
    /// 登记簿读写失败
    #[error("登记簿错误: {0}")]
    Registry(#[from] crate::hub::HubError),
    /// 键入参写法非法（形状规则见 key 模块）
    #[error(transparent)]
    InvalidKey(#[from] crate::key::KeyError),
    /// hub 未初始化或装配不完整（缺 .git / .gitignore / hub.toml）。
    /// 判定点住 git::open_hub，这里持它的展示形态——它是 hub 状态而非
    /// git 故障，故不套 `git 错误: ` 前缀
    #[error("hub 未初始化或装配不完整: {0}")]
    HubNotInitialized(String),
}

/// git 底层失败的收编：未初始化是 hub 状态，剥出独立变体，
/// 其余（tag 不存在等）按 git 故障原样包装
impl From<crate::git::GitError> for CommonError {
    fn from(e: crate::git::GitError) -> Self {
        match e {
            crate::git::GitError::HubNotInitialized(path) => CommonError::HubNotInitialized(path),
            other => CommonError::Git(other),
        }
    }
}

impl ErrorCode for CommonError {
    fn error_code(&self) -> &'static str {
        match self {
            CommonError::Io(_) => "io",
            CommonError::Git(g) => g.error_code(),
            CommonError::Lock(l) => l.error_code(),
            CommonError::Registry(h) => h.error_code(),
            CommonError::InvalidKey(k) => k.error_code(),
            CommonError::HubNotInitialized(_) => "hub_not_initialized",
        }
    }
}

/// 领域枚举吃下公共底层错误：本模块列出的底层类型在调用点 `?` 直用，
/// 转换只经公共底层错误这一条路径。只列该领域真正透传的类型
macro_rules! common_error_from {
    ($domain:ty: $($src:ty),+ $(,)?) => {
        $(
            impl From<$src> for $domain {
                fn from(e: $src) -> Self {
                    <$domain>::Common(crate::error::CommonError::from(e))
                }
            }
        )+
    };
}

pub(crate) use common_error_from;

#[cfg(test)]
mod tests {
    use super::*;

    /// 公共底层错误的稳定码与文案是对外契约：CLI 的 --json error.code 与
    /// GUI 的消费方都取自这里，改动即破坏消费方
    #[test]
    fn common_error_keeps_stable_codes_and_messages() {
        let io = CommonError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "缺文件"));
        assert_eq!(io.error_code(), "io");
        assert_eq!(io.to_string(), "文件系统错误: 缺文件");

        let git = CommonError::Git(crate::git::GitError::TagNotFound { tag: "v1".into() });
        assert_eq!(git.error_code(), "tag_not_found");
        assert_eq!(git.to_string(), "git 错误: tag 不存在: v1");

        let lock = CommonError::Lock(crate::lock::LockError::Held);
        assert_eq!(lock.error_code(), "lock_held");
        assert_eq!(
            lock.to_string(),
            "文件锁错误: hub 正被其他进程写入（文件锁被占用）"
        );

        let key = CommonError::InvalidKey(crate::key::KeyError {
            key: "a".into(),
            reason: "测试用原因",
        });
        assert_eq!(key.error_code(), "invalid_skill_key");
        assert_eq!(key.to_string(), "键非法: a（测试用原因）");
    }

    /// 透传：领域枚举照底层真因出稳定码与文案，包装层不改写格式化文案
    #[test]
    fn domain_error_passes_codes_and_messages_through() {
        let e = crate::skill::SkillError::from(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "缺文件",
        ));
        assert_eq!(e.error_code(), "io");
        assert_eq!(e.to_string(), "文件系统错误: 缺文件");

        let e = crate::project::ProjectError::from(crate::git::GitError::TagNotFound {
            tag: "v1".into(),
        });
        assert_eq!(e.error_code(), "tag_not_found");
        assert_eq!(e.to_string(), "git 错误: tag 不存在: v1");

        let e = crate::gc::GcError::from(crate::lock::LockError::Held);
        assert_eq!(e.error_code(), "lock_held");
        assert_eq!(
            e.to_string(),
            "文件锁错误: hub 正被其他进程写入（文件锁被占用）"
        );
    }

    /// 未初始化是 hub 状态而非 git 故障：收编 GitError 时剥出独立变体，
    /// 报错文案不带 `git 错误: ` 前缀，稳定码照旧
    #[test]
    fn hub_not_initialized_keeps_hub_status_message() {
        let raw = crate::git::GitError::HubNotInitialized("E:/hub".into());
        let common = CommonError::from(raw);
        assert!(matches!(common, CommonError::HubNotInitialized(_)));
        assert_eq!(common.error_code(), "hub_not_initialized");
        assert_eq!(common.to_string(), "hub 未初始化或装配不完整: E:/hub");

        // 领域枚举经同一条收编路径，文案逐字一致
        let e = crate::gc::GcError::from(crate::git::GitError::HubNotInitialized("E:/hub".into()));
        assert_eq!(e.error_code(), "hub_not_initialized");
        assert_eq!(e.to_string(), "hub 未初始化或装配不完整: E:/hub");
    }
}
