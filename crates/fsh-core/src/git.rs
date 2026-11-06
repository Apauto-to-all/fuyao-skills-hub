//! git 内部模块：对 libgit2 的薄封装，承载 hub 仓库的全部 git 操作。
//! 模块保持路径无关——只接收路径与仓库句柄，不含任何 hub 目录布局知识；
//! hub 布局（store\ / export\ 等）由上层调用方负责。
//! 并发控制：写函数（init / commit_all / tag_version）由上层入口在持有
//! 进程级文件锁（lock 模块）后调用，本模块不做自己的加锁。

use std::path::Path;
use std::time::Instant;

use git2::{
    Commit, IndexAddOption, Oid, Repository, RepositoryInitOptions, Signature, StatusOptions,
    build::CheckoutBuilder,
};
use thiserror::Error;

use crate::ErrorCode;

/// git 模块错误：聚合底层 git2 与 IO 错误，供上层结构化匹配
#[derive(Debug, Error)]
pub enum GitError {
    /// 文件系统操作失败（导出写盘、目录创建等）
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// libgit2 操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git2::Error),
    /// 目标目录已是一个 git 仓库，拒绝重复初始化
    #[error("目录已是 git 仓库: {0}")]
    AlreadyRepo(String),
    /// 指定的 tag 不存在
    #[error("tag 不存在: {tag}")]
    TagNotFound { tag: String },
}

impl ErrorCode for GitError {
    fn error_code(&self) -> &'static str {
        match self {
            GitError::Io(_) => "io",
            GitError::Git(_) => "git",
            GitError::AlreadyRepo(_) => "already_repo",
            GitError::TagNotFound { .. } => "tag_not_found",
        }
    }
}

/// 在指定目录初始化 git 仓库：初始分支 main，仓库本地 core.autocrlf=false
/// （防止换行改写污染内容哈希）。目录已含 .git 时拒绝初始化。
pub fn init(dir: &Path) -> Result<Repository, GitError> {
    let started = Instant::now();
    // 目录下已有 .git 视为既有仓库，拒绝重复初始化以防误操作破坏历史
    if dir.join(".git").exists() {
        return Err(GitError::AlreadyRepo(dir.display().to_string()));
    }
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    let repo = Repository::init_opts(dir, &opts)?;
    // 关闭换行改写：工作副本字节与内容树保持一致，内容哈希才稳定
    repo.config()?.set_str("core.autocrlf", "false")?;
    // 外部交互与耗时：仓库初始化是写入口，记子命令名与耗时供排查；失败由上层命令边界记一次
    tracing::debug!(
        op = %"init",
        path = %dir.display(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(repo)
}

/// 把工作区全部变更（含未跟踪文件）提交为一个 commit，返回新 commit id。
/// 提交身份由调用方传入（来源 hub.yaml），本模块不读任何全局配置。
pub fn commit_all(
    repo: &Repository,
    message: &str,
    name: &str,
    email: &str,
) -> Result<Oid, GitError> {
    let mut index = repo.index()?;
    // 全量纳入工作区变更：新增与修改一次进索引
    index.add_all(["*"].iter(), IndexAddOption::DEFAULT, None)?;
    index.write()?;
    commit_tree(repo, index.write_tree()?, message, name, email)
}

/// 只提交指定路径（相对仓库根）：修复类操作不得把工作区其它未提交改动卷进提交。
/// 这些路径相对 HEAD 没有变化时不产生提交。
pub fn commit_paths(
    repo: &Repository,
    paths: &[&Path],
    message: &str,
    name: &str,
    email: &str,
) -> Result<(), GitError> {
    let mut index = repo.index()?;
    for path in paths {
        index.add_path(path)?;
    }
    index.write()?;
    let tree_id = index.write_tree()?;
    // 与 HEAD 同一棵树 = 这几个文件没有可提交的变化
    if let Ok(head) = repo.head()
        && let Some(target) = head.target()
        && repo.find_commit(target)?.tree_id() == tree_id
    {
        return Ok(());
    }
    commit_tree(repo, tree_id, message, name, email)?;
    Ok(())
}

/// 用给定的树建一个 commit 并移动 HEAD，返回新 commit id
fn commit_tree(
    repo: &Repository,
    tree_id: Oid,
    message: &str,
    name: &str,
    email: &str,
) -> Result<Oid, GitError> {
    let started = Instant::now();
    let tree = repo.find_tree(tree_id)?;
    let sig = Signature::now(name, email)?;
    // 空仓库首次提交没有父提交
    let parent = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .map(|t| repo.find_commit(t))
        .transpose()?;
    let parents: Vec<&Commit<'_>> = parent.iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)?;
    // 外部交互与耗时：提交是写入口，只记子命令名与耗时，不记提交消息内容
    tracing::debug!(
        op = %"commit",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(oid)
}

/// 在 HEAD 上打版本 tag：`<skill>/v<semver>`。
pub fn tag_version(repo: &Repository, skill: &str, semver: &str) -> Result<Oid, GitError> {
    let started = Instant::now();
    let head = repo.head()?.peel_to_commit()?;
    let target = head.as_object();
    repo.tag_lightweight(&crate::naming::version_tag(skill, semver), target, true)?;
    // 外部交互与耗时：打 tag 是写入口，key / version 进字段，消息只说做了什么
    tracing::debug!(
        op = %"tag",
        key = %skill,
        version = %semver,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(head.id())
}

/// 版本条目：版本号、发布提交短哈希与提交时间
#[derive(Debug, Clone, PartialEq)]
pub struct TagEntry {
    /// 版本号（tag 名的尾段）
    pub version: String,
    /// 发布提交的短哈希（7 位）
    pub commit: String,
    /// 发布提交的时间（Unix 秒）
    pub date: u64,
}

/// 短哈希的固定位数：提交 id 恒为 40 位十六进制，截前 7 位足够区分
const SHORT_HASH_LEN: usize = 7;

/// 列出某 skill 的全部版本条目（新版本在前）：版本号、发布提交短哈希与提交时间。
/// 归属判定走 naming 的尾段语义：嵌套子键（如 a/b 之下的 a/b/vendor）的
/// 引用不属于本键
pub fn tag_entries(repo: &Repository, skill: &str) -> Result<Vec<TagEntry>, GitError> {
    let started = Instant::now();
    let mut entries: Vec<TagEntry> = Vec::new();
    // shortcut: 只认轻量 tag（oid 直接是提交）；出现注解 tag 时改走 resolve 剥离
    repo.tag_foreach(|oid, name| {
        let full = String::from_utf8_lossy(name);
        let Some((key, version)) = crate::naming::parse_version_tag_ref(&full) else {
            return true;
        };
        if key != skill {
            return true;
        }
        if let Ok(commit) = repo.find_commit(oid) {
            entries.push(TagEntry {
                version: version.to_string(),
                commit: commit.id().to_string()[..SHORT_HASH_LEN].to_string(),
                date: commit.time().seconds().max(0).cast_unsigned(),
            });
        }
        true
    })?;
    // 按语义化版本号降序：新版本在前
    entries.sort_by_key(|e| std::cmp::Reverse(crate::version::Version::parse(&e.version)));
    // 查询命中数与耗时：遍历全部 tag 后才知道本键有几个版本，命中数是排查时间线的关键
    tracing::debug!(
        op = %"tag_entries",
        key = %skill,
        count = entries.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(entries)
}

/// 列出某 skill 的全部版本号（新版本在前）。归属判定走 naming 的尾段
/// 语义：嵌套子键（如 a/b 之下的 a/b/vendor）的引用不属于本键
pub fn tag_list(repo: &Repository, skill: &str) -> Result<Vec<String>, GitError> {
    Ok(tag_entries(repo, skill)?
        .into_iter()
        .map(|e| e.version)
        .collect())
}

/// 解析 tag：返回其最终指向的 (commit id, 内容树 id)，调用方按需取用。
pub fn resolve(repo: &Repository, tag: &str) -> Result<(Oid, Oid), GitError> {
    // 解析失败（不存在 / 非 tag / 无法剥离到 commit）一律视为 tag 不存在
    let obj = repo
        .revparse_single(tag)
        .map_err(|_| GitError::TagNotFound {
            tag: tag.to_string(),
        })?;
    let commit = obj.peel_to_commit().map_err(|_| GitError::TagNotFound {
        tag: tag.to_string(),
    })?;
    let tree = commit.tree()?;
    Ok((commit.id(), tree.id()))
}

/// 把 tag 内容树中 subpath 子树导出到 dest 目录：剥掉子树前缀、不写仓库索引。
/// 导出产物是工作副本的还原来源，可随时删除后从 tag 重建。
pub fn materialize(
    repo: &Repository,
    tag: &str,
    subpath: &str,
    dest: &Path,
) -> Result<(), GitError> {
    let started = Instant::now();
    let (_, tree_id) = resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    // 子树剥成检出根：前缀天然剥除
    let subtree = tree
        .get_path(Path::new(subpath))?
        .to_object(repo)?
        .peel_to_tree()?;
    std::fs::create_dir_all(dest)?;
    let mut cb = CheckoutBuilder::new();
    // 导出到独立目录：不写仓库索引，工作区不受影响
    cb.target_dir(dest).force().update_index(false);
    repo.checkout_tree(subtree.as_object(), Some(&mut cb))?;
    // 外部交互与耗时：导出会落盘整棵子树，路径进字段便于定位产物；失败由上层边界记
    tracing::debug!(
        op = %"checkout",
        tag = %tag,
        path = %dest.display(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "git 命令执行"
    );
    Ok(())
}

/// 工作区是否有未提交变更（显式包含未跟踪文件）。
pub fn is_dirty(repo: &Repository) -> Result<bool, GitError> {
    let mut opts = StatusOptions::new();
    // 显式包含未跟踪文件，消除默认行为歧义
    opts.include_untracked(true);
    let dirty = !repo.statuses(Some(&mut opts))?.is_empty();
    Ok(dirty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 测试身份（模拟 hub.yaml 提供的提交者）
    const NAME: &str = "fuyao";
    const EMAIL: &str = "fuyao@local";

    /// 建一次性仓库：init 后写入 hub.yaml 骨架并首次提交
    fn setup() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = init(dir.path()).unwrap();
        fs::write(dir.path().join("hub.yaml"), "schema_version: 1\n").unwrap();
        commit_all(&repo, "init hub", NAME, EMAIL).unwrap();
        (dir, repo)
    }

    #[test]
    fn init_creates_main_branch_and_disables_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init(dir.path()).unwrap();
        // 初始分支必须是 main
        let head = repo.find_reference("HEAD").unwrap();
        assert_eq!(
            head.symbolic_target().unwrap(),
            Some("refs/heads/main"),
            "初始分支应为 main"
        );
        // 仓库本地必须关闭换行改写，否则内容哈希会被污染
        let cfg = repo.config().unwrap();
        let autocrlf = cfg.get_string("core.autocrlf").unwrap();
        assert_eq!(autocrlf.to_lowercase(), "false");
    }

    #[test]
    fn init_rejects_existing_repo() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();
        assert!(matches!(init(dir.path()), Err(GitError::AlreadyRepo(_))));
    }

    #[test]
    fn commit_all_includes_untracked_files() {
        let (dir, repo) = setup();
        // 新增未跟踪文件 + 修改已有文件，一次提交全进去
        fs::write(dir.path().join("new.txt"), "new").unwrap();
        fs::write(dir.path().join("hub.yaml"), "schema_version: 2\n").unwrap();
        let oid = commit_all(&repo, "second", NAME, EMAIL).unwrap();
        let commit = repo.find_commit(oid).unwrap();
        let tree = commit.tree().unwrap();
        assert!(tree.get_name("new.txt").is_some(), "未跟踪文件应被提交");
        let blob = tree
            .get_name("hub.yaml")
            .unwrap()
            .to_object(&repo)
            .unwrap()
            .peel_to_blob()
            .unwrap();
        assert_eq!(blob.content(), b"schema_version: 2\n");
        assert!(!is_dirty(&repo).unwrap(), "提交后工作区应干净");
    }

    #[test]
    fn tag_version_creates_version_tag_only() {
        let (dir, repo) = setup();
        let v1 = tag_version(&repo, "demo", "1.0.0").unwrap();
        let (commit, _) = resolve(&repo, "demo/v1.0.0").unwrap();
        assert_eq!(v1, commit, "版本 tag 应指向本次提交");

        // 追加提交后打 1.1.0：只多一个版本 tag，不产生移动指针
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        tag_version(&repo, "demo", "1.1.0").unwrap();
        assert_eq!(tag_list(&repo, "demo").unwrap(), vec!["1.1.0", "1.0.0"]);
    }

    #[test]
    fn tag_entries_carry_commit_and_date() {
        let (dir, repo) = setup();
        tag_version(&repo, "demo", "1.0.0").unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        commit_all(&repo, "second", NAME, EMAIL).unwrap();
        tag_version(&repo, "demo", "1.1.0").unwrap();

        let entries = tag_entries(&repo, "demo").unwrap();
        let versions: Vec<&str> = entries.iter().map(|e| e.version.as_str()).collect();
        assert_eq!(versions, vec!["1.1.0", "1.0.0"], "新版本在前");
        // 轻量 tag 没有 tagger 时间：日期取发布提交的提交时间
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        assert_eq!(
            entries[0].date,
            head.time().seconds().max(0).cast_unsigned()
        );
        assert_eq!(entries[0].commit, head.id().to_string()[..7]);
        assert!(
            entries.iter().all(|e| e.commit.len() == 7),
            "短哈希固定 7 位"
        );
    }

    #[test]
    fn tag_list_returns_versions_newest_first() {
        let (dir, repo) = setup();
        tag_version(&repo, "demo", "1.0.0").unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        commit_all(&repo, "second", NAME, EMAIL).unwrap();
        tag_version(&repo, "demo", "1.1.0").unwrap();
        assert_eq!(tag_list(&repo, "demo").unwrap(), vec!["1.1.0", "1.0.0"]);
    }

    #[test]
    fn resolve_reports_missing_tag() {
        let (_dir, repo) = setup();
        assert!(matches!(
            resolve(&repo, "demo/v9.9.9"),
            Err(GitError::TagNotFound { .. })
        ));
    }

    #[test]
    fn materialize_exports_subtree_without_prefix() {
        let (dir, repo) = setup();
        // 构造 store/demo/SKILL.md 工作副本并提交打 tag
        let skill_dir = dir.path().join("store").join("demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# demo\n").unwrap();
        commit_all(&repo, "add demo", NAME, EMAIL).unwrap();
        tag_version(&repo, "demo", "1.0.0").unwrap();

        // 导出 store/demo 子树到独立目录：前缀必须剥掉
        let dest = tempfile::tempdir().unwrap();
        materialize(&repo, "demo/v1.0.0", "store/demo", dest.path()).unwrap();
        let exported = fs::read_to_string(dest.path().join("SKILL.md"))
            .expect("导出产物不应带 store/demo 前缀");
        assert_eq!(exported, "# demo\n");
        // 导出不得弄脏仓库索引
        assert!(!is_dirty(&repo).unwrap());
    }

    #[test]
    fn is_dirty_detects_new_and_committed_states() {
        let (dir, repo) = setup();
        assert!(!is_dirty(&repo).unwrap(), "刚提交完应干净");
        fs::write(dir.path().join("untracked.txt"), "x").unwrap();
        assert!(is_dirty(&repo).unwrap(), "未跟踪文件应算脏");
    }
    #[test]
    fn tag_list_scopes_to_exact_key() {
        let (_dir, repo) = setup();
        tag_version(&repo, "a", "1.0.0").unwrap();
        tag_version(&repo, "a/b", "1.0.0").unwrap();
        tag_version(&repo, "a/b/vendor", "2.0.0").unwrap();
        assert_eq!(tag_list(&repo, "a").unwrap(), vec!["1.0.0"]);
        assert_eq!(tag_list(&repo, "a/b").unwrap(), vec!["1.0.0"]);
        assert_eq!(
            tag_list(&repo, "a/b/vendor").unwrap(),
            vec!["2.0.0"],
            "子键版本不得混进父键时间线"
        );
    }
}
