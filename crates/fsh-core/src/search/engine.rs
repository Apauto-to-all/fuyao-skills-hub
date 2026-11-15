//! 检索执行：白名单遍历、逐行匹配、键投影与恒定排序。

use std::io;
use std::path::Path;
use std::time::Instant;

use globset::{Glob, GlobSetBuilder};
use grep_matcher::{Match, Matcher};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{SearcherBuilder, sinks::Lossy};
use serde::{Deserialize, Serialize};

use super::SearchError;
use super::SearchOptions;
use super::scope::scope_prefix;
use crate::git;
use crate::paths::SKILL_FACES;

/// 缺省命中上限：截断只裁 hits，total 始终精确
pub const DEFAULT_LIMIT: usize = 200;

/// 单条命中（`key` 在 store 外命中时省略）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// 技能唯一键（命中在 store\<key>\ 之内才有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// 相对 hub 根路径（正斜杠）
    pub path: String,
    /// 行号（1 起）
    pub line: u64,
    /// 行文本（不含行终止符）
    pub text: String,
    /// 命中片段：行内字符偏移区间 [start, end)，按 Unicode 标量值计
    pub spans: Vec<(usize, usize)>,
}

/// 检索结果
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    /// 命中（恒定排序：路径升序、行号升序）
    pub hits: Vec<SearchHit>,
    /// 精确总数（不因截断变小）
    pub total: usize,
    /// hits 是否被 limit 截断
    pub truncated: bool,
}

/// 对 hub 根做内容检索；query 缺省按正则解释，字面量开关在选项里。
/// 命中不含上下文行，上下文由展示层经 context 模块独立展开
pub fn search(
    hub_root: &Path,
    query: &str,
    opts: &SearchOptions,
) -> Result<SearchResult, SearchError> {
    let started = Instant::now();
    git::open_hub(hub_root)?;
    // 遍历根恒为 hub 根：白名单 .gitignore 对任何调用形状都原样生效，
    // 范围与 glob 都不换根、不覆盖，退到命中过滤层
    let scope = scope_prefix(hub_root, opts.path.as_deref())?;
    let matcher = build_matcher(query, opts)?;
    let globs = build_globset(&opts.globs)?;
    let walker = build_walker(hub_root);
    // 单遍遍历：store 内含 SKILL.md 的目录随手收进技能目录集，
    // 命中先不带键收齐，全部遍历完再统一做「最近含 SKILL.md 祖先」投影
    let mut skill_dirs: Vec<String> = Vec::new();
    let mut raw_hits: Vec<SearchHit> = Vec::new();
    for entry in walker.build() {
        let entry = entry.map_err(|e| io::Error::other(e.to_string()))?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        if let Some(dir) = skill_dir_of(hub_root, entry.path()) {
            skill_dirs.push(dir);
        }
        search_file(hub_root, entry.path(), &matcher, &mut raw_hits)?;
    }
    skill_dirs.sort();
    let mut hits: Vec<SearchHit> = raw_hits
        .into_iter()
        .filter(|h| in_scope(&h.path, scope.as_deref()))
        .filter(|h| keep_by_globs(&h.path, &globs))
        .map(|mut h| {
            h.key = key_of(&h.path, &skill_dirs).map(str::to_string);
            h
        })
        .collect();
    // 恒定排序：路径升序、行号升序；遍历的目录序不外泄
    hits.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    let total = hits.len();
    let limit = opts.limit.unwrap_or(DEFAULT_LIMIT);
    let truncated = total > limit;
    hits.truncate(limit);
    tracing::debug!(
        count = total,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "内容检索完成"
    );
    Ok(SearchResult {
        hits,
        total,
        truncated,
    })
}

/// 匹配器：缺省 smart-case（模式全小写才忽略大小写），可强制忽略大小写、
/// 字面量与整词；模式编译失败报检索模式非法
fn build_matcher(query: &str, opts: &SearchOptions) -> Result<RegexMatcher, SearchError> {
    RegexMatcherBuilder::new()
        .case_insensitive(opts.case_insensitive)
        .case_smart(!opts.case_insensitive)
        .fixed_strings(opts.literal)
        .word(opts.whole_word)
        .build(query)
        .map_err(|e| SearchError::PatternInvalid(e.to_string()))
}

/// 遍历器：只认 hub 根 .gitignore 的白名单规则（store、agents、hub.toml
/// 可见，logs、.lock 与根级散件天然不可见）；根恒为 hub 根，
/// 范围与 glob 都不经此层
fn build_walker(root: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .standard_filters(false)
        .git_ignore(true)
        .require_git(true)
        .parents(false)
        .git_global(false)
        .git_exclude(false)
        .hidden(false);
    builder
}

/// glob 命中过滤层：正向并集保留、负向剔除，只能在白名单可见集内收窄
fn build_globset(globs: &[String]) -> Result<GlobFilter, SearchError> {
    let compile =
        |g: &str| Glob::new(g).map_err(|e| SearchError::PatternInvalid(format!("{g:?} {e}")));
    let mut pos = GlobSetBuilder::new();
    let mut neg = GlobSetBuilder::new();
    let mut has_pos = false;
    let mut has_neg = false;
    for glob in globs {
        // 负向 glob 剥掉前缀 ! 后按正向模式收进剔除集
        if let Some(pat) = glob.strip_prefix('!') {
            neg.add(compile(pat)?);
            has_neg = true;
        } else {
            pos.add(compile(glob)?);
            has_pos = true;
        }
    }
    let build = |b: GlobSetBuilder| {
        b.build()
            .map_err(|e| SearchError::PatternInvalid(e.to_string()))
    };
    Ok(GlobFilter {
        pos: has_pos.then(|| build(pos)).transpose()?,
        neg: has_neg.then(|| build(neg)).transpose()?,
    })
}

/// 命中过滤：正向 glob 存在则须至少命中一个；任一负向 glob 命中即剔除
fn keep_by_globs(path: &str, globs: &GlobFilter) -> bool {
    globs.is_empty()
        || (globs.pos.as_ref().is_none_or(|p| p.is_match(path))
            && globs.neg.as_ref().is_none_or(|p| !p.is_match(path)))
}

/// glob 过滤器（正向可选、负向可选）
struct GlobFilter {
    pos: Option<globset::GlobSet>,
    neg: Option<globset::GlobSet>,
}

impl GlobFilter {
    fn is_empty(&self) -> bool {
        self.pos.is_none() && self.neg.is_none()
    }
}

/// 范围过滤：命中路径须落在范围前缀（或即范围本身）之内
fn in_scope(path: &str, scope: Option<&str>) -> bool {
    scope.is_none_or(|s| path == s || path.starts_with(&format!("{s}/")))
}

/// store 与 archive 内含 SKILL.md 的目录（正斜杠相对 hub 根路径）；
/// 两个目录之外不构成技能
fn skill_dir_of(hub_root: &Path, abs: &Path) -> Option<String> {
    let rel = rel_path(hub_root, abs)?;
    let dir = rel.strip_suffix("/SKILL.md")?;
    for face in SKILL_FACES {
        if let Some(key) = dir.strip_prefix(&format!("{face}/"))
            && !key.is_empty()
        {
            return Some(format!("{face}/{key}"));
        }
    }
    None
}

/// 相对 hub 根的正斜杠路径；路径不在 hub 内时返回 None
fn rel_path(hub_root: &Path, abs: &Path) -> Option<String> {
    abs.strip_prefix(hub_root)
        .ok()
        .map(|r| r.to_string_lossy().replace('\\', "/"))
}

/// 键投影：路径取最近含 SKILL.md 的祖先（或自身）目录；
/// 找不到技能目录时省略键
fn key_of<'a>(path: &'a str, skill_dirs: &'a [String]) -> Option<&'a str> {
    let mut rest = path;
    let mut face = "";
    for f in SKILL_FACES {
        let pfx = format!("{f}/");
        if let Some(r) = path.strip_prefix(&pfx) {
            rest = r;
            face = f;
            break;
        }
    }
    if face.is_empty() {
        return None;
    }
    let mut best: Option<&str> = None;
    for dir in skill_dirs {
        // 目录集统一带面前缀；不同面的目录跳过（键在同面内比较）
        let Some(dir_key) = dir.strip_prefix(&format!("{face}/")) else {
            continue;
        };
        let prefix = format!("{dir_key}/");
        if rest == dir_key || rest.starts_with(&prefix) {
            // 目录已按字典序，最后留下的就是最长的（最近）祖先
            best = Some(dir_key);
        }
    }
    best
}

/// 单文件逐行匹配：命中行先不带键收齐（键统一在遍历后投影）
fn search_file(
    hub_root: &Path,
    abs: &Path,
    matcher: &RegexMatcher,
    hits: &mut Vec<SearchHit>,
) -> Result<(), SearchError> {
    let Some(path) = rel_path(hub_root, abs) else {
        return Ok(()); // 扫描根必在 hub 内，前缀缺失不发生
    };
    // 匹配期错误（模式已编译成功，实际不会再失败）经捕获位传出，
    // sink 的错误通道只承载文件系统错误
    let mut pending: Option<SearchError> = None;
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    searcher
        .search_path(
            matcher,
            abs,
            Lossy(|num, line| match spans(matcher, line) {
                Ok(sp) => {
                    hits.push(SearchHit {
                        key: None,
                        path: path.clone(),
                        line: num,
                        text: trim_eol(line).to_string(),
                        spans: sp,
                    });
                    Ok(true)
                }
                Err(e) => {
                    pending = Some(e);
                    Ok(false)
                }
            }),
        )
        .map_err(SearchError::from)?;
    match pending {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// 行内命中片段：字节区间换算成字符偏移区间 [start, end)
fn spans(matcher: &RegexMatcher, line: &str) -> Result<Vec<(usize, usize)>, SearchError> {
    let mut out = Vec::new();
    matcher
        .find_iter(line.as_bytes(), |m: Match| {
            let start = line[..m.start()].chars().count();
            let end = start + line[m.start()..m.end()].chars().count();
            out.push((start, end));
            true
        })
        .map_err(|e| SearchError::PatternInvalid(e.to_string()))?;
    Ok(out)
}

/// 去掉行终止符（\r 与 \n），行文本不含换行
fn trim_eol(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}
