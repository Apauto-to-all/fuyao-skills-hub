//! 库管理：skill 的入库（add）、发版（version bump）、两级删除（归档 /
//! purge）、恢复（restore）与查询（list / show / diff）。
//! 版本唯一实体是 git tag；工作副本是编辑面，物化目录是部署面——
//! 归档只删编辑面，版本链（tag）与部署面完好。
//! 全部写操作先取进程级文件锁（见 lock 模块），CLI 与 GUI 共用。
//! 本模块为目录模块：add / version / remove / restore / query 五个操作
//! 子模块共享此处的错误类型、公共类型与内部工具，对外 API 统一经此处重导出发布。

mod add;
mod query;
mod remove;
mod restore;
mod version;

// ---------- 公共 API ----------
pub use add::{add, add_dry};
pub use query::{diff, list, show};
pub use remove::{remove, remove_dry};
pub use restore::{restore, restore_dry};
pub use version::{bump_dry, version_bump};

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;
use crate::deploy;
use crate::git;
use crate::hub;
use crate::lock::{self, WriteToken};
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
    /// 配置 TOML 解析失败
    #[error("TOML 解析失败: {0}")]
    TomlDe(#[from] toml::de::Error),
    /// 配置 TOML 序列化失败
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
    /// skill 不存在（无工作副本也无版本）。键是 store 内相对路径
    #[error("skill 不存在: {key}")]
    SkillNotFound { key: String },
    /// 同键 skill 已存在（工作副本或版本 tag 占位）
    #[error("skill 已存在: {key}")]
    AlreadyExists { key: String },
    /// skill 处于归档态，该操作只对活跃 skill 开放
    #[error("skill 已归档: {key}")]
    Archived { key: String },
    /// 目标版本号不大于当前最新版本（号只能前进）
    #[error("目标版本 {target} 不大于当前最新版本 {current}，请改大后再发版: {key}")]
    VersionNotAdvancing {
        key: String,
        current: String,
        target: String,
    },
    /// 源目录不满足入库条件
    #[error("无效的 skill 源: {path}（{reason}）")]
    InvalidSource { path: String, reason: String },
    /// purge 被项目引用拒绝（附引用方项目路径）
    #[error("skill 仍被项目引用，先解链再删: {key}（{projects:?}）")]
    Referenced { key: String, projects: Vec<String> },
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
            SkillError::VersionNotAdvancing { .. } => "version_not_advancing",
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

/// SKILL.md frontmatter 顶层五字段：name / description / version / source / tags。
/// 其余顶层字段不解析也不写回，随内容字节透传（fsh 只替换 version 行）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Frontmatter {
    /// skill 名（= SKILL.md 所在目录名 = 唯一键末段）
    pub name: String,
    /// 一句话描述（入库必填）
    pub description: String,
    /// 发版输入：目标版本号（带引号的 x.y.z）；缺省时 add 取 0.1.0
    pub version: Option<String>,
    /// 外部来源（推荐 owner/repo 风格，不校验）；自产 skill 可缺省
    pub source: Option<String>,
    /// 自由标签（YAML 列表，流式 / 块式等价）
    pub tags: Vec<String>,
}

/// add 的结果报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddReport {
    /// 入库的 skill 唯一键（store 内相对路径）
    pub key: String,
    /// 首版版本号
    pub version: String,
}

/// remove 的结果报告
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoveReport {
    /// skill 唯一键
    pub key: String,
    /// 是否彻底删除（连版本 tag 一起）
    pub purged: bool,
}

/// list 的单行摘要
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillSummary {
    /// skill 唯一键（store 内相对路径）
    pub key: String,
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
    /// skill 唯一键
    pub key: String,
    /// 最新版本
    pub latest_version: Option<String>,
    /// 是否归档
    pub archived: bool,
    /// 编辑面 frontmatter 五字段（活跃态读工作副本，归档态读最新 tag 树；
    /// 读不到或解析不出来时为 None）
    pub frontmatter: Option<Frontmatter>,
    /// 版本时间线（全部版本 tag，新在前）
    pub versions: Vec<String>,
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
fn lock_and_open(hub_root: &Path) -> Result<(WriteToken, git2::Repository, HubPaths), SkillError> {
    let paths = HubPaths::from_root(hub_root);
    let guard = lock::write_lock(&paths)?;
    let repo = open_repo(hub_root)?;
    Ok((guard, repo, paths))
}

/// 提交身份：读 hub.toml；未配置时用默认身份
fn identity(paths: &HubPaths) -> Result<hub::UserIdentity, SkillError> {
    Ok(hub::load_config(paths)?.unwrap_or_default().user)
}

/// 读某键的 frontmatter：活跃态读工作副本，归档态读最新 tag 树里的 SKILL.md。
/// 读不到或解析不出来返回 None——查询面不因脏文件报错
fn frontmatter_of(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    versions: &[String],
) -> Result<Option<Frontmatter>, SkillError> {
    let label = format!("{}/SKILL.md", crate::naming::store_subpath(key));
    let text = if workdir_exists(paths, key) {
        match fs::read_to_string(paths.skill_workdir(key).join("SKILL.md")) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        }
    } else {
        let Some(latest) = versions.first() else {
            return Ok(None);
        };
        match skill_md_from_tag(repo, &crate::naming::version_tag(key, latest), key)? {
            Some(t) => t,
            None => return Ok(None),
        }
    };
    Ok(skill_identity(&text, &label).ok())
}

/// 无效 frontmatter / 源目录的统一错误构造
fn invalid(path: &str, reason: String) -> SkillError {
    SkillError::InvalidSource {
        path: path.to_string(),
        reason,
    }
}

// shortcut: 手写 YAML 子集（标量 + 流式/块式列表），不引入 YAML 依赖；
// 需要嵌套结构或完整 YAML 语法时换正经解析库
/// frontmatter 头部区：首行 --- 与下一个 --- 之间的行（不含定界行）
fn frontmatter_block(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut block = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Some(block);
        }
        block.push(line);
    }
    None
}

/// 取某顶层字段的原始行组：首行是冒号后的文本，后继行是它的续行
/// （缩进的块式列表，或与键同列的顶格 `- x` 列表项）；
/// 遇到下一个顶层字段即收束
fn field_lines<'a>(block: &[&'a str], key: &str) -> Option<Vec<&'a str>> {
    let mut group: Option<Vec<&'a str>> = None;
    for line in block {
        let top_level = !line.starts_with(' ') && !line.starts_with('\t');
        // 顶格 `- x` 是上一字段的块式列表项（YAML 允许列表项与键同列），
        // 不是新字段；其余顶格行一出现即说明本字段已收束
        if top_level && !line.starts_with('-') {
            if group.is_some() {
                return group;
            }
            let (k, v) = line.split_once(':').unwrap_or((line, ""));
            if k.trim() == key {
                group = Some(vec![v]);
            }
            continue;
        }
        if let Some(g) = group.as_mut() {
            g.push(line);
        }
    }
    group
}

/// 去引号与行尾注释：带引号取引号内文本，裸值截到 ` #`（YAML 注释起点）
fn unquote(raw: &str) -> String {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix('"') {
        return rest.split('"').next().unwrap_or_default().to_string();
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return rest.split('\'').next().unwrap_or_default().to_string();
    }
    match s.find(" #") {
        Some(i) => s[..i].trim_end().to_string(),
        None => s.to_string(),
    }
}

/// 标量字段值（name / description / source）：取首行并剥引号与注释
fn scalar_field(block: &[&str], key: &str) -> Option<String> {
    let group = field_lines(block, key)?;
    let value = unquote(group.first()?);
    (!value.is_empty()).then_some(value)
}

/// 列表字段值（tags）：流式 `[a, b]` 与块式 `- a` 等价，元素剥引号
fn list_field(block: &[&str], key: &str) -> Vec<String> {
    let Some(group) = field_lines(block, key) else {
        return Vec::new();
    };
    let head = unquote(group[0]);
    if let Some(inner) = head.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return inner
            .split(',')
            .map(unquote)
            .filter(|item| !item.is_empty())
            .collect();
    }
    group[1..]
        .iter()
        .filter_map(|line| line.trim().strip_prefix('-'))
        .map(unquote)
        .filter(|item| !item.is_empty())
        .collect()
}

/// 读 frontmatter 顶层五字段：缺 name/description、version 未带引号或非
/// 三段纯数字均报 InvalidSource；其余顶层字段不解析（写回也只碰 version 行）
fn skill_identity(text: &str, path: &str) -> Result<Frontmatter, SkillError> {
    let block = frontmatter_block(text)
        .ok_or_else(|| invalid(path, "缺少 frontmatter（--- 包围的头部区）".into()))?;
    let name = scalar_field(&block, "name")
        .ok_or_else(|| invalid(path, "frontmatter 缺少 name 字段".into()))?;
    let description = scalar_field(&block, "description")
        .ok_or_else(|| invalid(path, "frontmatter 缺少 description 字段".into()))?;
    Ok(Frontmatter {
        name,
        description,
        version: version_field(&block, path)?,
        source: scalar_field(&block, "source"),
        tags: list_field(&block, "tags"),
    })
}

/// version 字段：必须写成带引号的 x.y.z；裸值会被 YAML 当数字，号段被吃掉
fn version_field(block: &[&str], path: &str) -> Result<Option<String>, SkillError> {
    let Some(group) = field_lines(block, "version") else {
        return Ok(None);
    };
    let head = group[0].trim();
    if head.is_empty() {
        return Ok(None);
    }
    if !(head.starts_with('"') || head.starts_with('\'')) {
        return Err(invalid(
            path,
            format!("version 必须带引号（version: \"1.2.0\"）——裸值 {head} 会被 YAML 当数字解析"),
        ));
    }
    let value = unquote(head);
    if !valid_semver(&value) {
        return Err(invalid(
            path,
            format!("version 不是三段纯数字（x.y.z）: {value}"),
        ));
    }
    Ok(Some(value))
}

/// 校验 x.y.z 三段纯数字版本号（naming 的 tag 反解也用）
pub(crate) fn valid_semver(v: &str) -> bool {
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

/// 三段纯数字版本号比较（号只能前进；两侧都已过 valid_semver 校验）
fn cmp_semver(a: &str, b: &str) -> std::cmp::Ordering {
    let seg = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    seg(a).cmp(&seg(b))
}

/// 把目标版本号写回 frontmatter：只换 version 行的值，缺行则在头部区末尾补一行；
/// 其余字段与正文一个字节不改——透传字段不丢
fn set_frontmatter_version(text: &str, version: &str) -> Result<String, SkillError> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return Err(invalid(
            "SKILL.md",
            "缺少 frontmatter（--- 包围的头部区）".into(),
        ));
    }
    // 头部区收束在下一个 `---`；没有收束行即无 frontmatter
    let end = lines
        .iter()
        .skip(1)
        .position(|l| l.trim() == "---")
        .map(|i| i + 1)
        .ok_or_else(|| invalid("SKILL.md", "缺少 frontmatter（--- 包围的头部区）".into()))?;
    for i in 1..end {
        let line = &lines[i];
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let (k, _) = line.split_once(':').unwrap_or((line.as_str(), ""));
        // version 行由 fsh 独占：旧值、对齐与行尾注释一并让位给新号
        if k.trim() == "version" {
            let cr = if line.ends_with('\r') { "\r" } else { "" };
            lines[i] = format!("version: \"{version}\"{cr}");
            return Ok(lines.join("\n"));
        }
    }
    // 补在头部区末尾：不插进任何字段的行组里，块标量（`|` / `>`）不会被拆散；
    // 行尾沿用文件既有风格（CRLF 文件补出来的行也带 \r）
    let eol = if lines.iter().any(|l| l.ends_with('\r')) {
        "\r"
    } else {
        ""
    };
    lines.insert(end, format!("version: \"{version}\"{eol}"));
    Ok(lines.join("\n"))
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

/// 引用某 skill 的项目清单：走引用集（refs 模块）取引用方，
/// 不可达项目一律不贡献引用——与 GC / doctor 同一口径
fn referencing_projects(paths: &HubPaths, key: &str) -> Result<Vec<String>, SkillError> {
    Ok(crate::refs::reference_set(paths)?.referencing_projects(key))
}

/// 工作副本是否在盘
fn workdir_exists(paths: &HubPaths, key: &str) -> bool {
    paths.skill_workdir(key).is_dir()
}

/// 收集某键命名空间下的全部 tag 短名（版本 tag + stable 指针）。
/// 归属判定走 naming 的尾段语义：多级键下，嵌套子键的引用
/// （如 a/b 之下的 a/b/vendor/v1.0.0）不属于本键，purge 不得误删
fn skill_tags(repo: &git2::Repository, key: &str) -> Result<Vec<String>, SkillError> {
    let mut tags = Vec::new();
    repo.tag_foreach(|_oid, name| {
        let full = String::from_utf8_lossy(name);
        if crate::naming::key_from_tag_ref(&full) == Some(key) {
            tags.push(full.strip_prefix("refs/tags/").unwrap_or(&full).to_string());
        }
        true
    })?;
    Ok(tags)
}

/// 读 tag 内容树里 store/<键>/SKILL.md 的文本（归档态读 frontmatter 用）
fn skill_md_from_tag(
    repo: &git2::Repository,
    tag: &str,
    key: &str,
) -> Result<Option<String>, SkillError> {
    let (_, tree_id) = git::resolve(repo, tag)?;
    let tree = repo.find_tree(tree_id)?;
    let entry = match tree.get_path(Path::new(&format!(
        "{}/SKILL.md",
        crate::naming::store_subpath(key)
    ))) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    let obj = entry.to_object(repo)?;
    let blob = obj.peel_to_blob()?;
    Ok(Some(String::from_utf8_lossy(blob.content()).into_owned()))
}

/// 发版三连：以提交身份收编全部改动 → 打版本 tag → 物化到部署面。
/// 首版（add）与后续版（version --bump）共用，两条路径的发布语义永远一致。
/// 凭证参数保证两条发布路径都持全局写锁后进入。
fn release(
    repo: &git2::Repository,
    paths: &HubPaths,
    key: &str,
    version: &str,
    message: &str,
    _token: &WriteToken,
) -> Result<(), SkillError> {
    let id = identity(paths)?;
    git::commit_all(repo, message, &id.name, &id.email)?;
    git::tag_version(repo, key, version)?;
    git::materialize(
        repo,
        &crate::naming::version_tag(key, version),
        &crate::naming::store_subpath(key),
        &paths.materialized(key, version),
    )?;
    Ok(())
}
