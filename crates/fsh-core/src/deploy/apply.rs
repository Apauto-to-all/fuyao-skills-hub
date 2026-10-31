//! 落地执行：把 plan 产出的变更逐条物化、建链、记账，末尾顺手清
//! 本项目失引用且内容与 tag 一致的物化目录。

use std::fs;
use std::path::Path;

use super::DeployError;
use super::model::{
    LockEntry, StateLink, hash_dir_contents, hash_tag_contents, read_lock, read_state, write_lock,
    write_state,
};
use super::open_repo;
use super::plan::{Change, plan};
use crate::git;
use crate::link::{self, LinkKind};
use crate::paths::HubPaths;

/// 落点相对项目根的统一字符串形式（正斜杠，跨工具可读）
fn rel_dst(dst: &Path, project_dir: &Path) -> String {
    dst.strip_prefix(project_dir)
        .unwrap_or(dst)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 执行 plan 产出的全部变更：物化 + 建链 + 写账本/状态，末尾顺手清
/// 本项目已不再引用且内容与 tag 一致的物化目录。
pub fn apply(hub_root: &Path, project_dir: &Path) -> Result<Vec<Change>, DeployError> {
    let repo = open_repo(hub_root)?;
    let paths = HubPaths::from_root(hub_root);
    // 全局写锁：apply 动 hub 物化目录与登记簿，与其他写入口互斥
    let token = crate::lock::write_lock(&paths)?;
    let changes = plan(hub_root, project_dir)?;

    let mut lock = read_lock(project_dir)?;
    let mut state = read_state(project_dir)?;

    for change in &changes {
        // Install / Upgrade / Downgrade 的落盘动作相同：物化 → 链接 → 记账；
        // Noop 也要自愈——部署面（物化目录/链接）可能被删，账本对不上盘就补
        let (name, version, is_noop) = match change {
            Change::Install { name, version, .. }
            | Change::Upgrade {
                name, to: version, ..
            }
            | Change::Downgrade {
                name, to: version, ..
            } => (name, version, false),
            Change::Noop { name, version } => (name, version, true),
            Change::Remove { name, version } => {
                link::remove_link(&HubPaths::project_link_dst(project_dir, name))?;
                lock.entries.retain(|e| &e.name != name);
                state.links.retain(|l| &l.skill != name);
                let _ = version; // 账目按技能名清除，版本仅用于变更报告
                continue;
            }
        };

        let tag = crate::naming::version_tag(name, version);
        let subpath = crate::naming::store_subpath(name);
        let mat = paths.materialized(name, version);
        // 物化目录缺失或内容与 tag 不符时重物化（可删可重建是部署面的天性）
        if !mat.is_dir() || hash_dir_contents(&mat)? != hash_tag_contents(&repo, &tag, &subpath)? {
            if mat.exists() {
                fs::remove_dir_all(&mat)?;
            }
            git::materialize(&repo, &tag, &subpath, &mat)?;
        }
        let content_hash = hash_tag_contents(&repo, &tag, &subpath)?;

        // 链接落点：新装/升降级先清旧链再建新链；Noop 只在链接在位
        // 且指向仍是本次物化目录时续用（copy 无指向概念，在位即续用）
        let dst = HubPaths::project_link_dst(project_dir, name);
        let link_present = dst.symlink_metadata().is_ok() && dst.exists();
        let recorded_kind = lock
            .entries
            .iter()
            .find(|e| &e.name == name)
            .map(|e| e.kind);
        let points_to_mat =
            matches!((dst.canonicalize(), mat.canonicalize()), (Ok(a), Ok(b)) if a == b);
        let kind = if is_noop
            && link_present
            && (recorded_kind == Some(LinkKind::Copy) || points_to_mat)
        {
            recorded_kind.unwrap_or(LinkKind::Junction)
        } else {
            link::remove_link(&dst)?;
            link::create_dir_link(&mat, &dst, &content_hash)?
        };

        let (commit, _) = git::resolve(&repo, &tag)?;
        lock.entries.retain(|e| &e.name != name);
        lock.entries.push(LockEntry {
            name: name.clone(),
            version: version.clone(),
            commit: commit.to_string(),
            tree_hash: content_hash,
            kind,
            dst: rel_dst(&dst, project_dir),
        });
        state.links.retain(|l| &l.skill != name);
        state.links.push(StateLink {
            skill: name.clone(),
            version: version.clone(),
            kind,
            dst: rel_dst(&dst, project_dir),
            linked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
    }

    write_lock(project_dir, &lock, &token)?;
    write_state(project_dir, &state, &token)?;

    // 成功执行完毕：先把本项目登记进 hub 侧登记簿——顺手清的引用集
    // 要把刚写入的本项目 lock 计入，登记必须先行
    let mut registry = crate::hub::load_registry(&paths)?;
    registry.upsert(project_dir, crate::hub::unix_now());
    crate::hub::save_registry(&paths, &registry, &token)?;

    // 顺手清：本轮触达技能的物化目录按引用集（全登记簿口径，含他
    // 项目）即时清扫——他项目引用的目录跳过（INFO 日志可观测），
    // 分类与宽限规则与 gc 同源。清扫失败不回滚 apply 主体，残留交
    // gc 兜底。
    let mut touched: Vec<String> = Vec::new();
    for change in &changes {
        match change {
            Change::Install { name, .. }
            | Change::Upgrade { name, .. }
            | Change::Downgrade { name, .. }
            | Change::Remove { name, .. } => {
                if !touched.contains(name) {
                    touched.push(name.clone());
                }
            }
            Change::Noop { .. } => {}
        }
    }
    let _ = crate::gc::sweep_touched(&repo, &paths, &token, &touched);

    Ok(changes)
}
