//! 库管理：skill 的入库（add）、发版（version bump）、两级删除（归档 /
//! purge）、恢复（restore）与查询（list / show / diff）。
//! 版本唯一实体是 git tag；工作副本是编辑面，物化目录是部署面——
//! 归档只删编辑面，版本链（tag）与部署面完好。
//! 全部写操作先取进程级文件锁（见 lock 模块），CLI 与 GUI 共用。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use git2::DiffFormat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::deploy;
use crate::git;
use crate::hub;
use crate::lock::{self, LockGuard};
use crate::paths::HubPaths;

/// 库管理错误
#[derive(Debug, Error)]
pub enum SkillError {
    /// 文件系统操作失败
    #[error("文件系统错误: {0}")]
    Io(#[from] std::io::Error),
    /// git 底层操作失败
    #[error("git 错误: {0}")]
    Git(#[from] git::GitError),
    /// meta / 配置 TOML 解析失败
    #[error("TOML 解析失败: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// meta / 配置 TOML 序列化失败
    #[error("TOML 序列化失败: {0}")]
    TomlSer(#[from] toml::ser::Error),
    /// hub 未初始化（无 .git）
    #[error("hub 未初始化: {0}")]
    HubNotInitialized(String),
    /// 登记簿 / 配置读写失败
    #[error("hub 配置错误: {0}")]
    Hub(#[from] hub::HubError),
    /// 项目账本读取失败（引用查找）
    #[error("部署读取错误: {0}")]
    Deploy(#[from] deploy::DeployError),
    /// 文件锁获取失败
    #[error("文件锁错误: {0}")]
    Lock(#[from] lock::LockError),
    /// skill 不存在（无工作副本也无版本）
    #[error("skill 不存在: {skill}")]
    SkillNotFound { skill: String },
    /// 同名 skill 已存在（工作副本或版本 tag 占位）
    #[error("skill 已存在: {skill}")]
    AlreadyExists { skill: String },
    /// skill 处于归档态，该操作只对活跃 skill 开放
    #[error("skill 已归档: {skill}")]
    Archived { skill: String },
    /// 源目录不满足入库条件
    #[error("无效的 skill 源: {path}（{reason}）")]
    InvalidSource { path: String, reason: String },
    /// purge 被项目引用拒绝（附引用方项目路径）
    #[error("skill 仍被项目引用，先解链再删: {skill}（{projects:?}）")]
    Referenced {
        skill: String,
        projects: Vec<String>,
    },
}

impl ErrorCode for SkillError {
    fn error_code(&self) -> &'static str {
        match self {
            SkillError::Io(_) => "io",
            SkillError::Git(g) => g.error_code(),
            SkillError::TomlDe(_) => "toml_parse",
            SkillError::TomlSer(_) => "toml_serialize",
            SkillError::HubNotInitialized(_) => "hub_not_initialized",
            SkillError::Hub(h) => h.error_code(),
            SkillError::Deploy(d) => d.error_code(),
            SkillError::Lock(l) => l.error_code(),
            SkillError::SkillNotFound { .. } => "skill_not_found",
            SkillError::AlreadyExists { .. } => "already_exists",
            SkillError::Archived { .. } => "archived",
            SkillError::InvalidSource { .. } => "invalid_source",
            SkillError::Referenced { .. } => "referenced",
        }
    }
}

// git2 原生错误统一收口到 Git 变体，使 `?` 在全部 git2 调用点直接可用
impl From<git2::Error> for SkillError {
    fn from(e: git2::Error) -> Self {
        SkillError::Git(git::GitError::from(e))
    }
}

/// SKILL.md frontmatter：skill 的身份来源（name 必填，其余可选）
#[derive(Debug, Clone, PartialEq)]
pub struct Frontmatter {
    /// skill 名（= store 目录名 = tag 命名空间）
    pub name: String,
    /// 指定首版版本号（缺省 0.1.0）
    pub version: Option<String>,
    /// 一句话描述
    pub description: Option<String>,
}

/// meta\\<skill>.toml：skill 元数据（tracked）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SkillMeta {
    /// 入库来源（首次 add 时的源目录）
    pub source: String,
    /// 自由标签（list --tag 过滤用）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 作者
    #[serde(default)]
    pub author: Option<String>,
    /// 许可证
    #[serde(default)]
    pub license: Option<String>,
}

/// add 的结果报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddReport {
    /// 入库的 skill 名
    pub skill: String,
    /// 首版版本号
    pub version: String,
}

/// remove 的结果报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoveReport {
    /// skill 名
    pub skill: String,
    /// 是否彻底删除（连版本 tag 一起）
    pub purged: bool,
}

/// list 的单行摘要
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillSummary {
    /// skill 名
    pub name: String,
    /// 最新版本（无任何版本 tag 时为 None）
    pub latest_version: Option<String>,
    /// 是否归档（有版本无工作副本）
    pub archived: bool,
    /// 入库来源
    pub source: Option<String>,
}

/// show 的全量详情
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillDetail {
    /// skill 名
    pub name: String,
    /// 最新版本
    pub latest_version: Option<String>,
    /// 是否归档
    pub archived: bool,
    /// 元数据（meta 文件缺失时为 None）
    pub meta: Option<SkillMeta>,
    /// 版本时间线（全部版本 tag，新在前）
    pub versions: Vec<String>,
    /// frontmatter 描述
    pub description: Option<String>,
    /// 引用此 skill 的项目路径（登记簿 + 各项目 lock）
    pub referenced_by: Vec<String>,
}

/// diff 的单文件结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiffFile {
    /// 文件路径（相对 skill 根）
    pub path: String,
    /// unified diff 文本
    pub patch: String,
}

/// semver 段位（version --bump 的目标段）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpPart {
    /// 补丁位 x.y.Z
    Patch,
    /// 次版本位 x.Y.0
    Minor,
    /// 主版本位 X.0.0
    Major,
}

// ---------- 内部工具 ----------

/// 打开 hub 仓库；未初始化给出结构化错误
fn open_repo(hub_root: &Path) -> Result<git2::Repository, SkillError> {
    if !hub_root.join(".git").exists() {
        return Err(SkillError::HubNotInitialized(
            hub_root.display().to_string(),
        ));
    }
    Ok(git2::Repository::open(hub_root).map_err(git::GitError::from)?)
}

/// 取全局写锁并打开仓库（写操作统一入口姿势）
fn lock_and_open(hub_root: &Path) -> Result<(LockGuard, git2::Repository, HubPaths), SkillError> {
    let paths = HubPaths::from_root(hub_root);
    let guard = lock::write_lock(&paths)?;
    let repo = open_repo(hub_root)?;
    Ok((guard, repo, paths))
}

/// 提交身份：读 hub.toml；未配置时用默认身份
fn identity(paths: &HubPaths) -> Result<hub::UserIdentity, SkillError> {
    Ok(hub::load_config(paths)?.unwrap_or_default().user)
}

/// meta 文件路径
fn meta_path(paths: &HubPaths, skill: &str) -> PathBuf {
    paths.meta().join(format!("{skill}.toml"))
}

/// 读 meta；文件缺失返回 None
fn load_meta(paths: &HubPaths, skill: &str) -> Result<Option<SkillMeta>, SkillError> {
    match fs::read_to_string(meta_path(paths, skill)) {
        Ok(text) => Ok(Some(toml::from_str(&text)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 解析 SKILL.md frontmatter：首行 --- 到下一个 --- 之间的扁平 `key: value`。
// ponytail: 只解析扁平键值，不引入 YAML 依赖；嵌套结构超出 skill 声明需求
fn parse_frontmatter(text: &str) -> Option<BTreeMap<String, String>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut map = BTreeMap::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            return Some(map);
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    None
}

/// 从文本读出 skill 身份；frontmatter 缺失或无 name 视为无效
fn skill_identity(text: &str) -> Result<Frontmatter, SkillError> {
    let fm = parse_frontmatter(text).ok_or_else(|| SkillError::InvalidSource {
        path: "SKILL.md".into(),
        reason: "缺少 frontmatter（--- 包围的头部区）".into(),
    })?;
    let name = fm.get("name").cloned().filter(|n| !n.is_empty());
    let name = name.ok_or_else(|| SkillError::InvalidSource {
        path: "SKILL.md".into(),
        reason: "frontmatter 缺少 name 字段".into(),
    })?;
    Ok(Frontmatter {
        name,
        version: fm.get("version").cloned(),
        description: fm.get("description").cloned(),
    })
}

/// 校验 x.y.z 三段纯数字版本号
fn valid_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u64>().is_ok())
}

/// 版本号段位自增：x.y.z 按指定位 +1，低位归零
fn bump_semver(v: &str, part: BumpPart) -> Result<String, SkillError> {
    if !valid_semver(v) {
        return Err(SkillError::InvalidSource {
            path: "版本号".into(),
            reason: format!("不是合法的 x.y.z: {v}"),
        });
    }
    let mut seg: Vec<u64> = v.split('.').map(|p| p.parse().unwrap()).collect();
    match part {
        BumpPart::Patch => seg[2] += 1,
        BumpPart::Minor => {
            seg[1] += 1;
            seg[2] = 0;
        }
        BumpPart::Major => {
            seg[0] += 1;
            seg[1] = 0;
            seg[2] = 0;
        }
    }
    Ok(seg
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("."))
}

/// 递归复制目录树，跳过任何深度的 .git 目录（源自身是仓库时不带入嵌套仓库）
fn copy_skill_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_skill_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 引用某 skill 的项目清单：遍历登记簿全部项目，逐个读项目 lock 判引用
fn referencing_projects(paths: &HubPaths, skill: &str) -> Result<Vec<String>, SkillError> {
    let registry = hub::load_registry(paths)?;
    let mut out = Vec::new();
    for rec in &registry.projects {
        let refers = deploy::read_lock(Path::new(&rec.path))
            .map(|l| l.entries.iter().any(|e| e.name == skill))
            .unwrap_or(false);
        if refers {
            out.push(rec.path.clone());
        }
    }
    Ok(out)
}

/// 工作副本是否在盘
fn workdir_exists(paths: &HubPaths, skill: &str) -> bool {
    paths.skill_workdir(skill).is_dir()
}

/// 收集某 skill 命名空间下的全部 tag 短名（版本 tag + stable 指针）
fn skill_tags(repo: &git2::Repository, skill: &str) -> Result<Vec<String>, SkillError> {
    let prefix = format!("{skill}/");
    let mut tags = Vec::new();
    repo.tag_foreach(|_oid, name| {
        let short = String::from_utf8_lossy(name);
        if let Some(rest) = short.strip_prefix("refs/tags/")
            && rest.starts_with(&prefix)
        {
            tags.push(rest.to_string());
        }
        true
    })?;
    Ok(tags)
}

/// skill 全部名字：tag 命名空间 ∪ store 目录
fn all_skill_names(paths: &HubPaths, repo: &git2::Repository) -> Result<Vec<String>, SkillError> {
    let mut names: Vec<String> = Vec::new();
    let push = |n: &str, names: &mut Vec<String>| {
        if !n.is_empty() && !names.iter().any(|e| e == n) {
            names.push(n.to_string());
        }
    };
    repo.tag_foreach(|_oid, name| {
        if let Some(short) = String::from_utf8_lossy(name).strip_prefix("refs/tags/") {
            // 命名空间形如 <skill>/v... 或 <skill>/stable，取 / 前一段
            if let Some(ns) = short.split_once('/') {
                push(ns.0, &mut names);
            }
        }
        true
    })?;
    if let Ok(entries) = fs::read_dir(paths.store()) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                push(&e.file_name().to_string_lossy(), &mut names);
            }
        }
    }
    names.sort();
    Ok(names)
}

/// 读 tag 内容树里 store/<skill>/SKILL.md 的文本（归档态读 frontmatter 用）
fn skill_md_from_tag(
    repo: &git2::Repository,
    tag: &str,
    skill: &str,
) -> Result<Option<String>, SkillError> {
    let (_, tree_id) = git::resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let entry = match tree.get_path(Path::new(&format!("store/{skill}/SKILL.md"))) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let obj = entry.to_object(repo)?;
    let blob = obj.peel_to_blob()?;
    Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
}

// ---------- 公共 API ----------

/// 入库：本地 skill 目录 → 工作副本 + meta + commit + 首版 tag + 物化。
/// 名字唯一来源是 SKILL.md frontmatter；首版 = frontmatter version 缺省 0.1.0。
pub fn add(hub_root: &Path, src_dir: &Path) -> Result<AddReport, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let (name, version) = prepare_add(&repo, &paths, src_dir)?;

    copy_skill_tree(src_dir, &paths.skill_workdir(&name))?;
    let meta = SkillMeta {
        source: src_dir.display().to_string(),
        ..Default::default()
    };
    fs::write(meta_path(&paths, &name), toml::to_string(&meta)?)?;
    release(
        &repo,
        &paths,
        &name,
        &version,
        &format!("add {name} v{version}"),
    )?;
    Ok(AddReport {
        skill: name,
        version,
    })
}

/// 发版三连：以提交身份收编全部改动 → 打版本 tag → 物化到部署面。
/// 首版（add）与后续版（version --bump）共用，两条路径的发布语义永远一致。
fn release(
    repo: &git2::Repository,
    paths: &HubPaths,
    skill: &str,
    version: &str,
    message: &str,
) -> Result<(), SkillError> {
    let id = identity(paths)?;
    git::commit_all(repo, message, &id.name, &id.email)?;
    git::tag_version(repo, skill, version)?;
    git::materialize(
        repo,
        &format!("{skill}/v{version}"),
        &format!("store/{skill}"),
        &paths.materialized(skill, version),
    )?;
    Ok(())
}

/// add 预检：读身份、定首版、查重名；执行与 dry-run 共用同一套判定
fn prepare_add(
    repo: &git2::Repository,
    paths: &HubPaths,
    src_dir: &Path,
) -> Result<(String, String), SkillError> {
    let skill_md = src_dir.join("SKILL.md");
    let text = fs::read_to_string(&skill_md).map_err(|e| SkillError::InvalidSource {
        path: src_dir.display().to_string(),
        reason: format!("读不到 SKILL.md: {e}"),
    })?;
    let fm = skill_identity(&text)?;
    let version = fm.version.clone().unwrap_or_else(|| "0.1.0".to_string());
    if !valid_semver(&version) {
        return Err(SkillError::InvalidSource {
            path: src_dir.display().to_string(),
            reason: format!("frontmatter version 非法: {version}"),
        });
    }
    // 重名拒绝：工作副本、meta、版本 tag 任一占位都不允许覆盖
    if workdir_exists(paths, &fm.name)
        || meta_path(paths, &fm.name).exists()
        || !git::tag_list(repo, &fm.name)?.is_empty()
    {
        return Err(SkillError::AlreadyExists { skill: fm.name });
    }
    Ok((fm.name, version))
}

/// add 预演：与 add 同一套预检，返回将执行的动作清单，不落任何盘
pub fn add_dry(hub_root: &Path, src_dir: &Path) -> Result<Vec<String>, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let (name, version) = prepare_add(&repo, &paths, src_dir)?;
    Ok(vec![
        format!("copy {} -> store/{name}", src_dir.display()),
        format!("write meta/{name}.toml"),
        format!("commit: add {name} v{version}"),
        format!("tag {name}/v{version}"),
        format!("materialize store/{name}/{version}"),
    ])
}

/// 发版：把工作副本当前全部改动（含脏）打成新版本——commit + tag + 物化。
/// 只对活跃 skill 开放；归档 skill 无编辑面可提交，报错。
pub fn version_bump(hub_root: &Path, skill: &str, part: BumpPart) -> Result<String, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let (_current, next) = prepare_bump(&repo, &paths, skill, part)?;
    release(
        &repo,
        &paths,
        skill,
        &next,
        &format!("release {skill} v{next}"),
    )?;
    Ok(next)
}

/// 发版预检：确认 skill 活跃、算出下一版号；执行与 dry-run 共用同一套判定
fn prepare_bump(
    repo: &git2::Repository,
    paths: &HubPaths,
    skill: &str,
    part: BumpPart,
) -> Result<(String, String), SkillError> {
    let versions = git::tag_list(repo, skill)?;
    if !workdir_exists(paths, skill) {
        return Err(if versions.is_empty() {
            SkillError::SkillNotFound {
                skill: skill.into(),
            }
        } else {
            SkillError::Archived {
                skill: skill.into(),
            }
        });
    }
    let current = versions.first().ok_or(SkillError::SkillNotFound {
        skill: skill.into(),
    })?;
    let next = bump_semver(current, part)?;
    Ok((current.clone(), next))
}

/// 发版预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn bump_dry(hub_root: &Path, skill: &str, part: BumpPart) -> Result<Vec<String>, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let (current, next) = prepare_bump(&repo, &paths, skill, part)?;
    Ok(vec![
        format!("commit: release {skill} v{next}"),
        format!("tag {skill}/v{next}"),
        format!("materialize store/{skill}/{next}"),
        format!("stable 指针 {skill}/stable -> {skill}/v{next}"),
        format!("版本 {current} -> {next}"),
    ])
}

/// 两级删除：默认归档（删工作副本与 meta，版本 tag 与历史全留，
/// 供版不受影响）；purge = true 时连全部版本 tag 一起删（历史 commit
/// 保留，被项目引用则拒绝）。
pub fn remove(hub_root: &Path, skill: &str, purge: bool) -> Result<RemoveReport, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let prep = prepare_remove(&repo, &paths, skill, purge)?;

    let mut changed = false;
    if prep.had_workdir {
        fs::remove_dir_all(paths.skill_workdir(skill))?;
        changed = true;
    }
    if prep.had_meta {
        fs::remove_file(meta_path(&paths, skill))?;
        changed = true;
    }
    if changed {
        let id = identity(&paths)?;
        let verb = if purge { "purge" } else { "archive" };
        git::commit_all(&repo, &format!("{verb} {skill}"), &id.name, &id.email)?;
    }
    if purge {
        for tag in &prep.purge_tags {
            repo.tag_delete(tag)?;
        }
    }
    Ok(RemoveReport {
        skill: skill.into(),
        purged: purge,
    })
}

/// remove 预检结果：执行与 dry-run 共用
struct RemovePrep {
    /// 工作副本在场（将被删）
    had_workdir: bool,
    /// meta 在场（将被删）
    had_meta: bool,
    /// purge 时将删的该 skill 命名空间全部 tag（版本 + stable 指针）
    purge_tags: Vec<String>,
}

/// remove 预检：确认目标存在、purge 前查项目引用、收集将删 tag
fn prepare_remove(
    repo: &git2::Repository,
    paths: &HubPaths,
    skill: &str,
    purge: bool,
) -> Result<RemovePrep, SkillError> {
    let versions = git::tag_list(repo, skill)?;
    let had_workdir = workdir_exists(paths, skill);
    let had_meta = meta_path(paths, skill).exists();
    if versions.is_empty() && !had_workdir && !had_meta {
        return Err(SkillError::SkillNotFound {
            skill: skill.into(),
        });
    }
    // 彻底删除前必须无人引用：账本说有项目在用就拒绝，不给强制开关
    if purge {
        let projects = referencing_projects(paths, skill)?;
        if !projects.is_empty() {
            return Err(SkillError::Referenced {
                skill: skill.into(),
                projects,
            });
        }
    }
    let purge_tags = if purge {
        skill_tags(repo, skill)?
    } else {
        Vec::new()
    };
    Ok(RemovePrep {
        had_workdir,
        had_meta,
        purge_tags,
    })
}

/// remove 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn remove_dry(hub_root: &Path, skill: &str, purge: bool) -> Result<Vec<String>, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let prep = prepare_remove(&repo, &paths, skill, purge)?;
    let mut actions = Vec::new();
    if prep.had_workdir {
        actions.push(format!("remove store/{skill}"));
    }
    if prep.had_meta {
        actions.push(format!("remove meta/{skill}.toml"));
    }
    if prep.had_workdir || prep.had_meta {
        let verb = if purge { "purge" } else { "archive" };
        actions.push(format!("commit: {verb} {skill}"));
    }
    for tag in &prep.purge_tags {
        actions.push(format!("delete tag {tag}"));
    }
    Ok(actions)
}

/// 恢复：从最新版本 tag 把工作副本与 meta 取回编辑面，commit 一次，
/// 不打新 tag——版本链原样，只是编辑面复活。
pub fn restore(hub_root: &Path, skill: &str) -> Result<(), SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let latest = prepare_restore(&repo, &paths, skill)?;
    let tag = format!("{skill}/v{latest}");
    // 编辑面：物化路径直接写回工作副本目录
    git::materialize(
        &repo,
        &tag,
        &format!("store/{skill}"),
        &paths.skill_workdir(skill),
    )?;
    // meta：从 tag 内容树取 blob 写回
    if let Ok((_, tree_id)) = git::resolve(&repo, &tag) {
        let tree = repo.find_tree(tree_id)?;
        if let Ok(entry) = tree.get_path(Path::new(&format!("meta/{skill}.toml"))) {
            let obj = entry.to_object(&repo)?;
            let blob = obj.peel_to_blob()?;
            fs::create_dir_all(paths.meta())?;
            fs::write(meta_path(&paths, skill), blob.content())?;
        }
    }
    let id = identity(&paths)?;
    git::commit_all(&repo, &format!("restore {skill}"), &id.name, &id.email)?;
    Ok(())
}

/// restore 预检：有版本 tag 才可恢复、工作副本在场则拒绝；返回最新版本号。
/// 执行与 dry-run 共用同一套判定
fn prepare_restore(
    repo: &git2::Repository,
    paths: &HubPaths,
    skill: &str,
) -> Result<String, SkillError> {
    let versions = git::tag_list(repo, skill)?;
    if versions.is_empty() {
        return Err(SkillError::SkillNotFound {
            skill: skill.into(),
        });
    }
    if workdir_exists(paths, skill) {
        return Err(SkillError::AlreadyExists {
            skill: skill.into(),
        });
    }
    Ok(versions[0].clone())
}

/// restore 预演：同一套预检，返回将执行的动作清单，不落任何盘
pub fn restore_dry(hub_root: &Path, skill: &str) -> Result<Vec<String>, SkillError> {
    let (_guard, repo, paths) = lock_and_open(hub_root)?;
    let latest = prepare_restore(&repo, &paths, skill)?;
    Ok(vec![
        format!("checkout store/{skill} @ {skill}/v{latest}"),
        format!("restore meta/{skill}.toml"),
        format!("commit: restore {skill}"),
    ])
}

/// 库清单：全部 skill 的薄行摘要（含归档态）；tag_filter 按 meta tags 过滤
pub fn list(hub_root: &Path, tag_filter: Option<&str>) -> Result<Vec<SkillSummary>, SkillError> {
    let repo = open_repo(hub_root)?;
    let paths = HubPaths::from_root(hub_root);
    let mut out = Vec::new();
    for name in all_skill_names(&paths, &repo)? {
        let meta = load_meta(&paths, &name)?;
        if let Some(want) = tag_filter {
            let has = meta.as_ref().map(|m| m.tags.iter().any(|t| t == want));
            if has != Some(true) {
                continue;
            }
        }
        out.push(SkillSummary {
            latest_version: git::tag_list(&repo, &name)?.into_iter().next(),
            archived: !workdir_exists(&paths, &name),
            source: meta.map(|m| m.source),
            name,
        });
    }
    Ok(out)
}

/// 单个 skill 全量详情：meta、版本时间线、frontmatter 描述、被引用项目
pub fn show(hub_root: &Path, skill: &str) -> Result<SkillDetail, SkillError> {
    let repo = open_repo(hub_root)?;
    let paths = HubPaths::from_root(hub_root);
    let versions = git::tag_list(&repo, skill)?;
    let archived = !workdir_exists(&paths, skill);
    if versions.is_empty() && archived {
        return Err(SkillError::SkillNotFound {
            skill: skill.into(),
        });
    }
    // frontmatter：活跃态读工作副本，归档态读最新 tag 里的 SKILL.md
    let description = if !archived {
        fs::read_to_string(paths.skill_workdir(skill).join("SKILL.md"))
            .ok()
            .and_then(|t| skill_identity(&t).ok())
            .and_then(|fm| fm.description)
    } else {
        skill_md_from_tag(&repo, &format!("{skill}/v{}", versions[0]), skill)?
            .and_then(|t| skill_identity(&t).ok())
            .and_then(|fm| fm.description)
    };
    Ok(SkillDetail {
        latest_version: versions.first().cloned(),
        archived,
        meta: load_meta(&paths, skill)?,
        versions,
        description,
        referenced_by: referencing_projects(&paths, skill)?,
        name: skill.into(),
    })
}

/// 两版本差异：unified diff 按文件聚合
pub fn diff(hub_root: &Path, skill: &str, v1: &str, v2: &str) -> Result<Vec<DiffFile>, SkillError> {
    let repo = open_repo(hub_root)?;
    let (_, t1) = git::resolve(&repo, &format!("{skill}/v{v1}"))?;
    let (_, t2) = git::resolve(&repo, &format!("{skill}/v{v2}"))?;
    let tree1 = repo.find_tree(t1)?;
    let tree2 = repo.find_tree(t2)?;
    let diff = repo.diff_tree_to_tree(Some(&tree1), Some(&tree2), None)?;

    let mut files: Vec<DiffFile> = Vec::new();
    let mut current: Option<(String, String)> = None;
    // 只保留 skill 子树内的路径并剥掉 store/<skill>/ 前缀；
    // 树里其余部分（meta 等）的变更不属于该 skill 的版本内容差异
    let prefix = format!("store/{skill}/");
    diff.print(DiffFormat::Patch, |delta, _hunk, line| {
        // delta 非空表示进入新文件：把上一份落盘，开新一份
        if let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) {
            let path = path.to_string_lossy().into_owned();
            // 子树外的变更整份跳过：先落盘上一份，当前置空（不再累积内容行）
            let rel = path.strip_prefix(&prefix).map(str::to_string);
            // 同一文件的后续内容行不动缓冲；换文件（或出子树）才落盘重开
            let same_file = matches!((&current, &rel), (Some((p, _)), Some(r)) if p == r);
            if !same_file {
                if let Some(done) = current.take() {
                    files.push(DiffFile {
                        path: done.0,
                        patch: done.1,
                    });
                }
                if let Some(r) = rel {
                    current = Some((r, String::new()));
                }
            }
        }
        if let Some((_, buf)) = current.as_mut() {
            // 内容行的 +/-/空格 前缀在 origin 里，须手动拼回才是 unified diff 文本；
            // 文件头（F）与 hunk 头（H）行的 content 自身已完整
            match line.origin() {
                '+' | '-' | ' ' => {
                    buf.push(line.origin());
                    buf.push_str(&String::from_utf8_lossy(line.content()));
                }
                _ => buf.push_str(&String::from_utf8_lossy(line.content())),
            }
        }
        true
    })?;
    if let Some(done) = current.take() {
        files.push(DiffFile {
            path: done.0,
            patch: done.1,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deploy::{self, SkillsManifest};

    /// 建一次性 hub（初始化完成、工作区干净）
    fn setup() -> (tempfile::TempDir, HubPaths) {
        let install = tempfile::tempdir().unwrap();
        let paths = hub::hub_init(install.path()).unwrap();
        (install, paths)
    }

    /// hub 根路径（install\\data）
    fn root(install: &tempfile::TempDir) -> PathBuf {
        install.path().join("data")
    }

    /// 造一个源 skill 目录（带 frontmatter）
    fn make_skill(dir: &Path, name: &str, version: Option<&str>, desc: &str) -> PathBuf {
        let skill_dir = dir.join(name);
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        let fm_version = version
            .map(|v| format!("version: {v}\n"))
            .unwrap_or_default();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\n{fm_version}description: {desc}\n---\n\n# {name}\n正文\n"),
        )
        .unwrap();
        fs::write(skill_dir.join("references").join("a.txt"), "ref-a").unwrap();
        skill_dir
    }

    #[test]
    fn bump_semver_math() {
        assert_eq!(bump_semver("0.1.0", BumpPart::Patch).unwrap(), "0.1.1");
        assert_eq!(bump_semver("0.1.3", BumpPart::Minor).unwrap(), "0.2.0");
        assert_eq!(bump_semver("2.3.4", BumpPart::Major).unwrap(), "3.0.0");
        assert!(bump_semver("1.0", BumpPart::Patch).is_err());
        assert!(bump_semver("a.b.c", BumpPart::Patch).is_err());
    }

    #[test]
    fn add_creates_workdir_meta_tag_and_materialized() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        let src = make_skill(src_dir.path(), "demo", None, "示例技能");

        let report = add(&root(&install), &src).unwrap();
        assert_eq!(
            report,
            AddReport {
                skill: "demo".into(),
                version: "0.1.0".into()
            }
        );
        // 工作副本与 meta 落盘
        assert!(paths.skill_workdir("demo").join("SKILL.md").is_file());
        assert!(meta_path(&paths, "demo").is_file());
        // 首版 tag 与物化目录
        let repo = git2::Repository::open(paths.root()).unwrap();
        assert_eq!(git::tag_list(&repo, "demo").unwrap(), vec!["0.1.0"]);
        assert!(
            paths
                .materialized("demo", "0.1.0")
                .join("SKILL.md")
                .is_file()
        );
        // 提交完成、工作区干净
        assert!(!git::is_dirty(&repo).unwrap());
    }

    #[test]
    fn add_honors_frontmatter_version() {
        let (install, _paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        let src = make_skill(src_dir.path(), "ver-skill", Some("2.1.0"), "带版本");
        let report = add(&root(&install), &src).unwrap();
        assert_eq!(report.version, "2.1.0");
    }

    #[test]
    fn add_rejects_invalid_sources() {
        let (install, _paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        // 无 SKILL.md
        let empty = src_dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(matches!(
            add(&root(&install), &empty),
            Err(SkillError::InvalidSource { .. })
        ));
        // SKILL.md 无 frontmatter
        let no_fm = src_dir.path().join("no-fm");
        fs::create_dir_all(&no_fm).unwrap();
        fs::write(no_fm.join("SKILL.md"), "# 只有正文\n").unwrap();
        assert!(matches!(
            add(&root(&install), &no_fm),
            Err(SkillError::InvalidSource { .. })
        ));
        // frontmatter 无 name
        let no_name = src_dir.path().join("no-name");
        fs::create_dir_all(&no_name).unwrap();
        fs::write(no_name.join("SKILL.md"), "---\ndescription: 无名\n---\n").unwrap();
        assert!(matches!(
            add(&root(&install), &no_name),
            Err(SkillError::InvalidSource { .. })
        ));
    }

    #[test]
    fn add_rejects_duplicate_by_workdir_or_tag() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        let src = make_skill(src_dir.path(), "dup", None, "重复");
        add(&root(&install), &src).unwrap();
        // 工作副本占位 → 拒绝
        assert!(matches!(
            add(&root(&install), &src),
            Err(SkillError::AlreadyExists { .. })
        ));
        // 归档后（无工作副本、有 tag）→ 仍然拒绝
        remove(&root(&install), "dup", false).unwrap();
        assert!(!paths.skill_workdir("dup").exists());
        assert!(matches!(
            add(&root(&install), &src),
            Err(SkillError::AlreadyExists { .. })
        ));
    }

    #[test]
    fn add_excludes_nested_git_dir() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        let src = make_skill(src_dir.path(), "nested", None, "嵌套仓库");
        fs::create_dir_all(src.join(".git").join("objects")).unwrap();
        fs::write(src.join(".git").join("HEAD"), "ref: refs/heads/main").unwrap();
        add(&root(&install), &src).unwrap();
        assert!(!paths.skill_workdir("nested").join(".git").exists());
        assert!(
            paths
                .skill_workdir("nested")
                .join("references")
                .join("a.txt")
                .is_file()
        );
    }

    #[test]
    fn bump_collects_dirty_changes_into_new_version() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "grow", None, "成长"),
        )
        .unwrap();
        // 工作副本改脏（编辑面随便改是常态）
        fs::write(
            paths.skill_workdir("grow").join("SKILL.md"),
            "---\nname: grow\ndescription: 成长 v2\n---\n\n新版正文\n",
        )
        .unwrap();
        let next = version_bump(&root(&install), "grow", BumpPart::Minor).unwrap();
        assert_eq!(next, "0.2.0");
        let repo = git2::Repository::open(paths.root()).unwrap();
        assert_eq!(
            git::tag_list(&repo, "grow").unwrap(),
            vec!["0.2.0", "0.1.0"]
        );
        assert!(paths.materialized("grow", "0.2.0").is_dir());
        assert!(!git::is_dirty(&repo).unwrap(), "脏改动应被收编进新版本");
        // 新 tag 的 SKILL.md 内容 = 改后版本
        let text = skill_md_from_tag(&repo, "grow/v0.2.0", "grow")
            .unwrap()
            .unwrap();
        assert!(text.contains("新版正文"));
    }

    #[test]
    fn bump_rejects_archived_or_missing() {
        let (install, _paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "gone", None, "归档"),
        )
        .unwrap();
        remove(&root(&install), "gone", false).unwrap();
        assert!(matches!(
            version_bump(&root(&install), "gone", BumpPart::Patch),
            Err(SkillError::Archived { .. })
        ));
        assert!(matches!(
            version_bump(&root(&install), "never", BumpPart::Patch),
            Err(SkillError::SkillNotFound { .. })
        ));
    }

    #[test]
    fn archive_removes_workdir_but_keeps_versions_and_supply() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "arch", None, "归档供版"),
        )
        .unwrap();
        remove(&root(&install), "arch", false).unwrap();
        // 编辑面与 meta 删除
        assert!(!paths.skill_workdir("arch").exists());
        assert!(!meta_path(&paths, "arch").exists());
        // 版本链全在、历史提交可查
        let repo = git2::Repository::open(paths.root()).unwrap();
        assert_eq!(git::tag_list(&repo, "arch").unwrap(), vec!["0.1.0"]);
        assert!(!git::is_dirty(&repo).unwrap());
        // 归档不阻塞供版：项目声明它 → apply 照常铺出
        let proj = tempfile::tempdir().unwrap();
        let manifest = SkillsManifest {
            skills: vec![deploy::SkillReq {
                name: "arch".into(),
                version: "0.1.0".into(),
                target: "neutral".into(),
            }],
        };
        fs::write(
            HubPaths::project_manifest(proj.path()),
            toml::to_string(&manifest).unwrap(),
        )
        .unwrap();
        deploy::apply(paths.root(), proj.path()).unwrap();
        let lock = deploy::read_lock(proj.path()).unwrap();
        assert!(
            lock.entries.iter().any(|e| e.name == "arch"),
            "归档 skill 仍应可 apply"
        );
    }

    #[test]
    fn purge_removes_tags_and_rejects_when_referenced() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "dead", None, "待purge"),
        )
        .unwrap();
        // 项目在用 → purge 拒绝
        let proj = tempfile::tempdir().unwrap();
        let manifest = SkillsManifest {
            skills: vec![deploy::SkillReq {
                name: "dead".into(),
                version: "0.1.0".into(),
                target: "neutral".into(),
            }],
        };
        fs::write(
            HubPaths::project_manifest(proj.path()),
            toml::to_string(&manifest).unwrap(),
        )
        .unwrap();
        deploy::apply(paths.root(), proj.path()).unwrap();
        assert!(matches!(remove(&root(&install), "dead", true),
            Err(SkillError::Referenced { skill, .. }) if skill == "dead"));
        // 解链后 purge 成功：tag 清空、提交历史仍在
        fs::remove_file(HubPaths::project_manifest(proj.path())).unwrap();
        deploy::apply(paths.root(), proj.path()).unwrap();
        remove(&root(&install), "dead", true).unwrap();
        let repo = git2::Repository::open(paths.root()).unwrap();
        assert!(git::tag_list(&repo, "dead").unwrap().is_empty());
        assert!(git::resolve(&repo, "dead/stable").is_err());
        assert!(repo.head().is_ok(), "hub 仓库历史应完好");
        // 再 purge / remove 同名 → 不存在
        assert!(matches!(
            remove(&root(&install), "dead", true),
            Err(SkillError::SkillNotFound { .. })
        ));
    }

    #[test]
    fn restore_recovers_workdir_without_new_tag() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        let src = make_skill(src_dir.path(), "back", None, "复活");
        add(&root(&install), &src).unwrap();
        // 编辑面（工作副本）改出 v2 内容后发版，归档，再恢复
        fs::write(
            paths.skill_workdir("back").join("SKILL.md"),
            "---\nname: back\ndescription: 复活 v2\n---\n\nv2 正文\n",
        )
        .unwrap();
        version_bump(&root(&install), "back", BumpPart::Major).unwrap();
        remove(&root(&install), "back", false).unwrap();
        restore(&root(&install), "back").unwrap();
        // 工作副本按最新 tag 内容复活、meta 回来
        let text = fs::read_to_string(paths.skill_workdir("back").join("SKILL.md")).unwrap();
        assert!(text.contains("v2 正文"));
        assert!(meta_path(&paths, "back").is_file());
        // 版本链原样（无新 tag）、工作区干净
        let repo = git2::Repository::open(paths.root()).unwrap();
        assert_eq!(
            git::tag_list(&repo, "back").unwrap(),
            vec!["1.0.0", "0.1.0"]
        );
        assert!(!git::is_dirty(&repo).unwrap());
    }

    #[test]
    fn restore_rejects_active_or_unknown() {
        let (install, _paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "live", None, "活跃"),
        )
        .unwrap();
        assert!(matches!(
            restore(&root(&install), "live"),
            Err(SkillError::AlreadyExists { .. })
        ));
        assert!(matches!(
            restore(&root(&install), "ghost"),
            Err(SkillError::SkillNotFound { .. })
        ));
    }

    #[test]
    fn list_shows_rows_with_archive_flag_and_tag_filter() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "alpha", None, "甲"),
        )
        .unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "beta", None, "乙"),
        )
        .unwrap();
        remove(&root(&install), "beta", false).unwrap();
        // 给 alpha 补 tags 便于过滤（直接改 meta 再 add 无必要——手写 meta 测试数据）
        let meta = SkillMeta {
            source: "somewhere".into(),
            tags: vec!["rust".into()],
            author: None,
            license: None,
        };
        fs::write(meta_path(&paths, "alpha"), toml::to_string(&meta).unwrap()).unwrap();

        let rows = list(&root(&install), None).unwrap();
        let alpha = rows.iter().find(|r| r.name == "alpha").unwrap();
        let beta = rows.iter().find(|r| r.name == "beta").unwrap();
        assert_eq!(alpha.latest_version.as_deref(), Some("0.1.0"));
        assert!(!alpha.archived);
        assert!(beta.archived, "归档后 list 应标 archived");
        // tag 过滤
        let filtered = list(&root(&install), Some("rust")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "alpha");
        let none = list(&root(&install), Some("nope")).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn show_returns_full_detail_with_references() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        add(
            &root(&install),
            &make_skill(src_dir.path(), "full", None, "全量详情"),
        )
        .unwrap();
        // 项目 apply 产生引用
        let proj = tempfile::tempdir().unwrap();
        let manifest = SkillsManifest {
            skills: vec![deploy::SkillReq {
                name: "full".into(),
                version: "0.1.0".into(),
                target: "neutral".into(),
            }],
        };
        fs::write(
            HubPaths::project_manifest(proj.path()),
            toml::to_string(&manifest).unwrap(),
        )
        .unwrap();
        deploy::apply(paths.root(), proj.path()).unwrap();

        let d = show(&root(&install), "full").unwrap();
        assert_eq!(d.latest_version.as_deref(), Some("0.1.0"));
        assert!(!d.archived);
        assert_eq!(d.versions, vec!["0.1.0"]);
        assert_eq!(d.description.as_deref(), Some("全量详情"));
        assert_eq!(d.referenced_by.len(), 1);
        // 归档后仍可 show（描述读自最新 tag）
        remove(&root(&install), "full", false).unwrap();
        let d2 = show(&root(&install), "full").unwrap();
        assert!(d2.archived);
        assert_eq!(d2.description.as_deref(), Some("全量详情"));
        assert!(matches!(
            show(&root(&install), "ghost"),
            Err(SkillError::SkillNotFound { .. })
        ));
    }

    #[test]
    fn diff_reports_per_file_unified_patch() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        let src = make_skill(src_dir.path(), "delta", None, "差异");
        add(&root(&install), &src).unwrap();
        // 改 SKILL.md + 新增文件，打包 0.2.0
        fs::write(
            paths.skill_workdir("delta").join("SKILL.md"),
            "---\nname: delta\ndescription: 差异\n---\n\n新正文\n",
        )
        .unwrap();
        fs::write(paths.skill_workdir("delta").join("NEW.md"), "新文件").unwrap();
        version_bump(&root(&install), "delta", BumpPart::Minor).unwrap();

        let files = diff(&root(&install), "delta", "0.1.0", "0.2.0").unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(names.contains(&"NEW.md"));
        assert!(names.contains(&"SKILL.md"));
        let skill_patch = files.iter().find(|f| f.path == "SKILL.md").unwrap();
        assert!(skill_patch.patch.contains("-正文"));
        assert!(skill_patch.patch.contains("+新正文"));
        // 同版本 diff 为空
        assert!(
            diff(&root(&install), "delta", "0.2.0", "0.2.0")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn dry_runs_plan_actions_without_touching_disk() {
        let (install, paths) = setup();
        let src_dir = tempfile::tempdir().unwrap();
        let src = make_skill(src_dir.path(), "peek", None, "预演");
        // add 预演：动作清单齐全，store 一字未动
        let actions = add_dry(&root(&install), &src).unwrap();
        assert!(actions.iter().any(|a| a.contains("tag peek/v0.1.0")));
        assert!(
            actions
                .iter()
                .any(|a| a.contains("materialize store/peek/0.1.0"))
        );
        assert!(paths.store().read_dir().unwrap().next().is_none());

        add(&root(&install), &src).unwrap();
        // bump 预演：报出 0.1.0 -> 0.2.0，不产生新 tag
        let actions = bump_dry(&root(&install), "peek", BumpPart::Minor).unwrap();
        assert!(actions.iter().any(|a| a.contains("0.1.0 -> 0.2.0")));
        assert_eq!(
            git::tag_list(&git2::Repository::open(paths.root()).unwrap(), "peek").unwrap(),
            vec!["0.1.0"]
        );

        // remove 预演（purge）：列出将删 tag，盘上工作副本与 tag 都还在
        let actions = remove_dry(&root(&install), "peek", true).unwrap();
        assert!(actions.iter().any(|a| a == "remove store/peek"));
        assert!(actions.iter().any(|a| a == "delete tag peek/v0.1.0"));
        assert!(paths.skill_workdir("peek").is_dir());

        remove(&root(&install), "peek", false).unwrap();
        // restore 预演：点名最新 tag，工作副本仍未复活
        let actions = restore_dry(&root(&install), "peek").unwrap();
        assert!(
            actions
                .iter()
                .any(|a| a == "checkout store/peek @ peek/v0.1.0")
        );
        assert!(!paths.skill_workdir("peek").is_dir());
    }
}
