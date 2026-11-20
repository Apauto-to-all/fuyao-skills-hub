//! 列表按套收窄：`--prefix` 只列该套技能，与位置过滤词取交集。

mod common;

use std::fs;
use std::path::Path;

use common::{Run, data_of, init_hub, json};

/// 造一个源技能目录并入库到指定前缀
fn add_skill(hub: &str, src_root: &Path, name: &str, prefix: Option<&str>) {
    let dir = src_root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} 的说明\n---\n\n正文\n"),
    )
    .unwrap();
    let dir = dir.display().to_string();
    let mut args = vec!["add", dir.as_str()];
    if let Some(prefix) = prefix {
        args.extend(["--prefix", prefix]);
    }
    let Run { stdout, code, .. } = json(hub, &args);
    assert_eq!(code, 0, "add {name}: {stdout}");
}

/// 列表按套收窄：只看该前缀下（含更深层）的技能，与位置过滤词取交集
#[test]
fn list_prefix_narrows_to_the_set() {
    let install = tempfile::tempdir().unwrap();
    let src_root = tempfile::tempdir().unwrap();
    let hub = init_hub(&install);
    add_skill(&hub, src_root.path(), "tdd", Some("k1/k2"));
    add_skill(&hub, src_root.path(), "sql", Some("k1/k2/db"));
    add_skill(&hub, src_root.path(), "other", Some("k1/k2b"));
    add_skill(&hub, src_root.path(), "demo", None);

    let keys = |hub: &str, args: &[&str]| {
        let Run { stdout, code, .. } = json(hub, args);
        assert_eq!(code, 0, "{args:?}: {stdout}");
        data_of(&stdout)["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["key"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };

    assert_eq!(
        keys(&hub, &["list", "--prefix", "k1/k2"]),
        vec!["k1/k2/db/sql".to_string(), "k1/k2/tdd".to_string()],
        "只列这一套，含更深层，不含相邻前缀"
    );
    assert_eq!(
        keys(&hub, &["list", "--prefix", "k1/k2", "both-missing"]).len(),
        0,
        "过滤词不命中就一行不出"
    );
    assert_eq!(
        keys(&hub, &["list", "--prefix", "k1/k2", "tdd"]),
        vec!["k1/k2/tdd".to_string()],
        "与位置过滤词取交集"
    );
    assert_eq!(
        keys(&hub, &["list", "--prefix", "k1/k2b"]),
        vec!["k1/k2b/other".to_string()],
        "相邻前缀是另一套"
    );
}
