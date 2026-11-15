//! hub.toml 读写与默认值。

use crate::hub::load_config;
use crate::paths::HubPaths;

#[test]
fn load_config_returns_none_when_missing() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    assert!(load_config(&paths).unwrap().is_none());
}

#[test]
fn config_parse_error_names_its_own_file() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::from_root(install.path());
    std::fs::write(paths.hub_toml(), "schema_version = ").unwrap();
    let err = load_config(&paths).unwrap_err();
    assert!(
        err.to_string().starts_with("hub.toml 解析失败: "),
        "应点名 hub.toml 且只写一次前缀: {err}"
    );
}
