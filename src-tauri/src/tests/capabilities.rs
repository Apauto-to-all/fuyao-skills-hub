//! capabilities 快照测试：初始化判定只认全套装配标记。

use super::*;
use std::fs;

#[test]
fn snapshot_reports_initialized_only_when_artifacts_exist() {
    let dir = std::env::temp_dir().join("fsh-gui-capabilities-test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("建临时目录");
    let paths = HubPaths::from_root(&dir);

    let before = snapshot(&paths);
    assert!(!before.initialized);
    assert_eq!(before.hub_root, paths.root().display().to_string());

    // 只有 hub.toml 不算已初始化：半截装配必须暴露给引导页，否则用户
    // 看到的界面全是「已就绪」，点任何动作都回 hub_not_initialized
    fs::write(paths.hub_toml(), "").expect("写 hub.toml");
    assert!(!snapshot(&paths).initialized);

    fs::write(paths.gitignore(), "").expect("写 .gitignore");
    fs::create_dir_all(paths.root().join(".git")).expect("建 .git");
    assert!(snapshot(&paths).initialized);

    let _ = fs::remove_dir_all(&dir);
}
