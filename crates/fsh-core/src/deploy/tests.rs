use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use crate::hub;
use crate::link::LinkKind;
use crate::paths::HubPaths;

/// 测试用 hub + 项目：hub 初始化完毕并入库一个 demo 技能两个版本
fn setup_hub_with_skill() -> (tempfile::TempDir, tempfile::TempDir) {
    let install = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let paths = hub::hub_init(install.path()).unwrap();

    // 工作副本两个版本演进：1.0.0 → 2.0.0
    let work = paths.skill_workdir("demo");
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("SKILL.md"), "# v1\n").unwrap();
    let repo = git2::Repository::open(paths.root()).unwrap();
    git::commit_all(&repo, "demo v1", "fuyao", "fuyao@local").unwrap();
    git::tag_version(&repo, "demo", "1.0.0").unwrap();
    fs::write(work.join("SKILL.md"), "# v2\n").unwrap();
    git::commit_all(&repo, "demo v2", "fuyao", "fuyao@local").unwrap();
    git::tag_version(&repo, "demo", "2.0.0").unwrap();
    (install, project)
}

/// 写项目侧 skills.toml
fn write_manifest(project: &Path, reqs: &[SkillReq]) {
    let m = SkillsManifest {
        skills: reqs.to_vec(),
    };
    fs::write(
        HubPaths::project_manifest(project),
        toml::to_string(&m).unwrap(),
    )
    .unwrap();
}

fn req(name: &str, version: &str) -> SkillReq {
    SkillReq {
        name: name.into(),
        version: version.into(),
        target: "claude-code".into(),
    }
}

fn lock_of(project: &Path) -> LockFile {
    toml::from_str(&fs::read_to_string(HubPaths::project_lock(project)).unwrap()).unwrap()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn plan_covers_all_five_change_kinds() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();

    // 空声明 + 空账本：无变更
    assert!(plan(&hub_root, project.path()).unwrap().is_empty());

    // 新装
    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    let changes = plan(&hub_root, project.path()).unwrap();
    assert_eq!(
        changes,
        vec![Change::Install {
            name: "demo".into(),
            version: "1.0.0".into(),
            target: "claude-code".into()
        }]
    );

    // 手工构造账本模拟「已装 1.0.0」
    let lock = LockFile {
        entries: vec![LockEntry {
            name: "demo".into(),
            version: "1.0.0".into(),
            commit: "0".repeat(40),
            tree_hash: "x".into(),
            kind: LinkKind::Junction,
            dst: ".fsh/skills/demo".into(),
        }],
    };
    fs::write(
        HubPaths::project_lock(project.path()),
        toml::to_string(&lock).unwrap(),
    )
    .unwrap();

    // 已满足
    assert_eq!(
        plan(&hub_root, project.path()).unwrap(),
        vec![Change::Noop {
            name: "demo".into(),
            version: "1.0.0".into()
        }]
    );
    // 升级
    write_manifest(project.path(), &[req("demo", "2.0.0")]);
    assert_eq!(
        plan(&hub_root, project.path()).unwrap(),
        vec![Change::Upgrade {
            name: "demo".into(),
            from: "1.0.0".into(),
            to: "2.0.0".into(),
            target: "claude-code".into()
        }]
    );
    // 降级：账本视为已装 2.0.0，声明要 1.0.0
    let mut lock2 = lock.clone();
    lock2.entries[0].version = "2.0.0".into();
    fs::write(
        HubPaths::project_lock(project.path()),
        toml::to_string(&lock2).unwrap(),
    )
    .unwrap();
    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    assert_eq!(
        plan(&hub_root, project.path()).unwrap(),
        vec![Change::Downgrade {
            name: "demo".into(),
            from: "2.0.0".into(),
            to: "1.0.0".into(),
            target: "claude-code".into()
        }]
    );
    // 撤销：声明清空但账本有
    write_manifest(project.path(), &[]);
    assert_eq!(
        plan(&hub_root, project.path()).unwrap(),
        vec![Change::Remove {
            name: "demo".into(),
            version: "2.0.0".into()
        }]
    );
    // plan 全程不动盘：账本没被改写
    assert_eq!(lock_of(project.path()), lock2);
}

#[test]
fn plan_resolves_stable_to_newest() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    write_manifest(project.path(), &[req("demo", "stable")]);
    let changes = plan(&hub_root, project.path()).unwrap();
    assert_eq!(
        changes,
        vec![Change::Install {
            name: "demo".into(),
            version: "2.0.0".into(),
            target: "claude-code".into()
        }]
    );
}

#[test]
fn plan_reports_missing_version() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    write_manifest(project.path(), &[req("demo", "9.9.9")]);
    assert!(matches!(
        plan(&hub_root, project.path()),
        Err(DeployError::VersionNotFound { .. })
    ));
}

#[test]
fn apply_installs_materializes_links_and_records() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    write_manifest(project.path(), &[req("demo", "1.0.0")]);

    let applied = apply(&hub_root, project.path()).unwrap();
    assert_eq!(applied.len(), 1);
    assert!(matches!(applied[0], Change::Install { .. }));

    // 物化目录存在且内容正确（剥子树前缀）
    let paths = HubPaths::new(install.path());
    let mat = paths.materialized("demo", "1.0.0");
    assert_eq!(fs::read_to_string(mat.join("SKILL.md")).unwrap(), "# v1\n");
    // 链接落点可读
    let dst = HubPaths::project_link_dst(project.path(), "demo");
    assert!(dst.join("SKILL.md").is_file());
    // 账本记满四要素
    let lock = lock_of(project.path());
    assert_eq!(lock.entries.len(), 1);
    let e = &lock.entries[0];
    assert_eq!(e.name, "demo");
    assert_eq!(e.version, "1.0.0");
    assert_ne!(e.commit, "0".repeat(40), "应记真实 commit id");
    assert!(!e.tree_hash.is_empty());
    assert_eq!(e.dst, ".fsh/skills/demo");
    // state.toml 有链接记录
    let state: ProjectState =
        toml::from_str(&fs::read_to_string(HubPaths::project_state(project.path())).unwrap())
            .unwrap();
    assert_eq!(state.links.len(), 1);
    assert!(state.links[0].linked_at <= now());

    // 幂等：重跑全是 Noop 且零副作用
    let again = apply(&hub_root, project.path()).unwrap();
    assert_eq!(
        again,
        vec![Change::Noop {
            name: "demo".into(),
            version: "1.0.0".into()
        }]
    );
    assert_eq!(lock_of(project.path()).entries.len(), 1);

    // apply 后 status 零漂移
    assert!(status(&hub_root, project.path()).unwrap().is_empty());
}

#[test]
fn apply_upgrade_relinks_and_gc_sweeps_old() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    let paths = HubPaths::new(install.path());

    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    apply(&hub_root, project.path()).unwrap();
    assert!(paths.materialized("demo", "1.0.0").is_dir());

    // 升到 2.0.0：旧物化目录被顺手清（本项目无引用、内容与 tag 一致）
    write_manifest(project.path(), &[req("demo", "2.0.0")]);
    let applied = apply(&hub_root, project.path()).unwrap();
    assert!(matches!(applied[0], Change::Upgrade { .. }));
    assert!(
        !paths.materialized("demo", "1.0.0").exists(),
        "无引用旧物化应被清"
    );
    assert!(paths.materialized("demo", "2.0.0").is_dir());
    // 链接指向新版本内容
    let dst = HubPaths::project_link_dst(project.path(), "demo");
    assert!(dst.join("SKILL.md").is_file());
    assert_eq!(lock_of(project.path()).entries[0].version, "2.0.0");
    assert!(status(&hub_root, project.path()).unwrap().is_empty());
}

#[test]
fn apply_rematerializes_when_dir_missing() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    let paths = HubPaths::new(install.path());

    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    apply(&hub_root, project.path()).unwrap();
    // 人为删掉物化目录（模拟 GC / git clean）
    fs::remove_dir_all(paths.materialized("demo", "1.0.0")).unwrap();
    // status 报物化缺失（链接悬空随之暴露，两条都是事实）
    let drifts = status(&hub_root, project.path()).unwrap();
    assert!(
        drifts.contains(&Drift::MaterialMismatch {
            name: "demo".into(),
            version: "1.0.0".into()
        }),
        "应检出物化缺失: {drifts:?}"
    );
    // apply 自动重物化并归位
    apply(&hub_root, project.path()).unwrap();
    assert!(paths.materialized("demo", "1.0.0").is_dir());
    assert!(status(&hub_root, project.path()).unwrap().is_empty());
}

#[test]
fn apply_remove_unlinks_and_cleans_entry() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    apply(&hub_root, project.path()).unwrap();

    write_manifest(project.path(), &[]);
    let applied = apply(&hub_root, project.path()).unwrap();
    assert!(matches!(applied[0], Change::Remove { .. }));
    assert!(
        !HubPaths::project_link_dst(project.path(), "demo").exists(),
        "链接应撤销"
    );
    assert!(lock_of(project.path()).entries.is_empty(), "账目应清除");
    let paths = HubPaths::new(install.path());
    assert!(
        !paths.materialized("demo", "1.0.0").exists(),
        "无引用物化应被顺手清"
    );
}

#[test]
fn status_detects_project_side_drifts() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    apply(&hub_root, project.path()).unwrap();
    let dst = HubPaths::project_link_dst(project.path(), "demo");

    // 链接被删
    let meta = fs::symlink_metadata(&dst).unwrap();
    if meta.file_type().is_symlink() {
        fs::remove_dir(&dst).unwrap();
    } else {
        fs::remove_dir_all(&dst).unwrap();
    }
    assert_eq!(
        status(&hub_root, project.path()).unwrap(),
        vec![Drift::LinkMissing {
            name: "demo".into()
        }]
    );

    // 恢复后 copy 模式篡改内容：只有拷贝才能篡改；非拷贝模式跳过此断言
    apply(&hub_root, project.path()).unwrap();
    if lock_of(project.path()).entries[0].kind == LinkKind::Copy {
        fs::write(dst.join("SKILL.md"), "tampered\n").unwrap();
        assert_eq!(
            status(&hub_root, project.path()).unwrap(),
            vec![Drift::CopyModified {
                name: "demo".into()
            }]
        );
    }
}

#[test]
fn status_detects_hub_side_dirty() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    apply(&hub_root, project.path()).unwrap();

    // 弄脏工作副本
    let paths = HubPaths::new(install.path());
    fs::write(paths.skill_workdir("demo").join("SKILL.md"), "hand edit\n").unwrap();
    let drifts = status(&hub_root, project.path()).unwrap();
    assert!(
        drifts.contains(&Drift::HubDirty),
        "应检出 HubDirty: {drifts:?}"
    );
}

#[test]
fn status_and_apply_heal_relinked_pointing_elsewhere() {
    let (install, project) = setup_hub_with_skill();
    let hub_root = HubPaths::new(install.path()).root().to_path_buf();
    write_manifest(project.path(), &[req("demo", "1.0.0")]);
    apply(&hub_root, project.path()).unwrap();
    let dst = HubPaths::project_link_dst(project.path(), "demo");
    if lock_of(project.path()).entries[0].kind == LinkKind::Copy {
        // copy 模式无指向概念，此路径不适用
        return;
    }

    // 外部把链接重指到别的目录
    crate::link::remove_link(&dst).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    junction::create(elsewhere.path(), &dst).unwrap();
    assert!(
        status(&hub_root, project.path())
            .unwrap()
            .contains(&Drift::LinkMissing {
                name: "demo".into()
            }),
        "重指的链接应按 LinkMissing 检出"
    );

    // 重跑 apply 自愈：链接指回物化目录，漂移清零
    apply(&hub_root, project.path()).unwrap();
    assert_eq!(status(&hub_root, project.path()).unwrap(), Vec::new());
}
