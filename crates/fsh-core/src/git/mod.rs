//! git 内部模块：对 libgit2 的薄封装，承载 hub 仓库的全部 git 操作。
//! 模块保持路径无关——只接收路径与仓库句柄，不含任何 hub 目录布局知识；
//! hub 布局（store\ / meta\ / cache\ 等）由上层调用方负责。

use std::path::Path;

use git2::{
    Commit, IndexAddOption, ObjectType, Oid, Repository, RepositoryInitOptions, Signature,
    StatusOptions, TreeEntry, TreeWalkMode, build::CheckoutBuilder,
};
use thiserror::Error;

/// git 模块错误：聚合底层 git2 与 IO 错误，供上层结构化匹配
#[derive(Debug, Error)]
pub enum GitError {
    /// 文件系统操作失败（物化写盘、目录创建等）
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

/// 在指定目录初始化 git 仓库：初始分支 main，仓库本地 core.autocrlf=false
/// （防止换行改写污染内容哈希）。目录必须是空目录或不存在。
// TODO: M1 在此与全部写操作入口加进程级文件锁，堵 GUI 与 CLI 并发写 index.lock
pub fn init(dir: &Path) -> Result<Repository, GitError> {
    // 目录下已有 .git 视为既有仓库，拒绝重复初始化以防误操作破坏历史
    if dir.join(".git").exists() {
        return Err(GitError::AlreadyRepo(dir.display().to_string()));
    }
    let mut opts = RepositoryInitOptions::new();
    opts.initial_head("main");
    let repo = Repository::init_opts(dir, &opts)?;
    // 关闭换行改写：工作副本字节与内容树保持一致，内容哈希才稳定
    repo.config()?.set_str("core.autocrlf", "false")?;
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
    let tree = repo.find_tree(index.write_tree()?)?;
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
    Ok(oid)
}

/// 在 HEAD 上打版本 tag：`<skill>/v<semver>`，并把 `<skill>/stable` 指针
/// 强制移到同一 commit（可移动稳定指针，等价 git tag -f）。
pub fn tag_version(repo: &Repository, skill: &str, semver: &str) -> Result<Oid, GitError> {
    let head = repo.head()?.peel_to_commit()?;
    let target = head.as_object();
    // 版本 tag 与 stable 指针都 force 覆盖：stable 始终指向最新版本
    repo.tag_lightweight(&format!("{skill}/v{semver}"), target, true)?;
    repo.tag_lightweight(&format!("{skill}/stable"), target, true)?;
    Ok(head.id())
}

/// 列出某 skill 的全部版本号（剥掉 `<skill>/v` 前缀，新版本在前）。
pub fn tag_list(repo: &Repository, skill: &str) -> Result<Vec<String>, GitError> {
    let prefix = format!("{skill}/v");
    let mut versions = Vec::new();
    repo.tag_foreach(|_oid, name| {
        let name = String::from_utf8_lossy(name);
        // 回调收到的是完整引用名（refs/tags/...），先剥引用前缀再剥版本前缀；
        // stable 指针不带 /v 前缀，天然不会混入
        if let Some(v) = name
            .strip_prefix("refs/tags/")
            .and_then(|n| n.strip_prefix(&prefix))
        {
            versions.push(v.to_string());
        }
        true
    })?;
    // 按语义化版本号降序：新版本在前；逐段数字比较，避免字符串排序错位
    let key = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    versions.sort_by_key(|v| std::cmp::Reverse(key(v)));
    Ok(versions)
}

/// 解析 tag：返回其最终指向的 (commit id, 内容树 id)。
/// lock 记账（记 commit）与漂移检测（比树）共用。
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

/// 把 tag 内容树中 subpath 子树物化到 dest 目录：剥掉子树前缀、不写仓库索引。
/// 物化目录是部署面缓存，可随时删除后重建。
pub fn materialize(
    repo: &Repository,
    tag: &str,
    subpath: &str,
    dest: &Path,
) -> Result<(), GitError> {
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
    Ok(())
}

/// 计算 tag 内容树的内容哈希（blake3）：按确定性顺序遍历，喂入
/// 「相对路径 + blob id」。同内容必同值；漂移检测与 GC 删除前校验共用。
pub fn tree_hash(repo: &Repository, tag: &str) -> Result<String, GitError> {
    let (_, tree_id) = resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let mut hasher = blake3::Hasher::new();
    // 先序遍历：顺序只由内容树条目决定，与文件系统无关；喂入相对路径 + blob id
    tree.walk(
        TreeWalkMode::PreOrder,
        &mut |dir: &str, entry: &TreeEntry<'_>| {
            if entry.kind() == Some(ObjectType::Blob) {
                let name = entry.name().unwrap_or("");
                let path = if dir.is_empty() {
                    name.to_string()
                } else {
                    format!("{dir}/{name}")
                };
                hasher.update(path.as_bytes());
                hasher.update(entry.id().as_bytes());
            }
            0
        },
    )?;
    Ok(hasher.finalize().to_hex().to_string())
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
    fn tag_version_creates_version_and_moves_stable() {
        let (_dir, repo) = setup();
        let v1 = tag_version(&repo, "demo", "1.0.0").unwrap();
        let (stable_commit, _) = resolve(&repo, "demo/stable").unwrap();
        assert_eq!(v1, stable_commit, "stable 应指向 1.0.0 的 commit");

        // 追加提交后打 1.1.0，stable 必须跟着移动
        fs::write(_dir.path().join("b.txt"), "b").unwrap();
        let head2 = commit_all(&repo, "second", NAME, EMAIL).unwrap();
        let v2 = tag_version(&repo, "demo", "1.1.0").unwrap();
        assert_eq!(v2, head2);
        let (stable_commit2, _) = resolve(&repo, "demo/stable").unwrap();
        assert_eq!(v2, stable_commit2, "stable 应移动到 1.1.0");
    }

    #[test]
    fn tag_list_returns_versions_newest_first() {
        let (_dir, repo) = setup();
        tag_version(&repo, "demo", "1.0.0").unwrap();
        fs::write(_dir.path().join("b.txt"), "b").unwrap();
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

        // 物化 store/demo 子树到独立目录：前缀必须剥掉
        let dest = tempfile::tempdir().unwrap();
        materialize(&repo, "demo/v1.0.0", "store/demo", dest.path()).unwrap();
        let exported = fs::read_to_string(dest.path().join("SKILL.md"))
            .expect("物化产物不应带 store/demo 前缀");
        assert_eq!(exported, "# demo\n");
        // 物化不得弄脏仓库索引
        assert!(!is_dirty(&repo).unwrap());
    }

    #[test]
    fn tree_hash_stable_and_content_sensitive() {
        let (dir, repo) = setup();
        let skill_dir = dir.path().join("store").join("demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# v1\n").unwrap();
        commit_all(&repo, "v1", NAME, EMAIL).unwrap();
        tag_version(&repo, "demo", "1.0.0").unwrap();
        let h1 = tree_hash(&repo, "demo/v1.0.0").unwrap();
        // 同 tag 重复计算必须稳定
        assert_eq!(h1, tree_hash(&repo, "demo/v1.0.0").unwrap());

        // 内容变化后 hash 必须变化
        fs::write(skill_dir.join("SKILL.md"), "# v2 content\n").unwrap();
        commit_all(&repo, "v2", NAME, EMAIL).unwrap();
        tag_version(&repo, "demo", "2.0.0").unwrap();
        assert_ne!(h1, tree_hash(&repo, "demo/v2.0.0").unwrap());
    }

    #[test]
    fn is_dirty_detects_new_and_committed_states() {
        let (dir, repo) = setup();
        assert!(!is_dirty(&repo).unwrap(), "刚提交完应干净");
        fs::write(dir.path().join("untracked.txt"), "x").unwrap();
        assert!(is_dirty(&repo).unwrap(), "未跟踪文件应算脏");
    }
}
