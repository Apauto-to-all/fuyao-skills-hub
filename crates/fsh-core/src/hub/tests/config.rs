//! hub.toml 读写与默认值。

use crate::hub::{HubConfig, UserIdentity, load_config};
use crate::paths::HubPaths;

#[test]
fn load_config_returns_none_when_missing() {
    let install = tempfile::tempdir().unwrap();
    let paths = HubPaths::new(install.path());
    assert!(load_config(&paths).unwrap().is_none());
}

#[test]
fn config_toml_roundtrip_keeps_all_fields() {
    let mut cfg = HubConfig::v1();
    cfg.user.name = "alice".to_string();
    cfg.gc.grace_days = 7;
    let text = cfg.to_toml().unwrap();
    let back = HubConfig::from_toml(&text).unwrap();
    assert_eq!(back, cfg);
    // 缺省字段（无 user/gc 段）落默认值
    let minimal = HubConfig::from_toml("schema_version = 1\n").unwrap();
    assert_eq!(minimal.user, UserIdentity::default());
    assert_eq!(minimal.gc.grace_days, 0);
}
