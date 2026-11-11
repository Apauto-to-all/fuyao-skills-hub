//! hub 定位链（进程级）：FSH_HUB 覆盖缺省、--hub 覆盖 FSH_HUB。
//! 缺省家目录分支不在子进程里测（不碰真实家目录），
//! 由 fsh-core 路径族单测的注入式形态覆盖。

mod common;

use common::{env_with_source, fsh_env, json_env};

#[test]
fn env_hub_directs_commands_without_flag() {
    let (install, _) = env_with_source();
    let hub = install.path().display().to_string();
    // 只设环境变量：不带 --hub 的命令也全部落在 FSH_HUB 指定的根上
    let r = fsh_env(&[("FSH_HUB", &hub)], &["--json", "init"]);
    assert_eq!(r.code, 0, "init via FSH_HUB: {}", r.stdout);
    assert!(
        install.path().join("store").is_dir(),
        "hub 应装配在 FSH_HUB 指向的根下"
    );
}

#[test]
fn hub_flag_overrides_env_hub() {
    let (install, _) = env_with_source();
    let (flag_root, _) = env_with_source();
    let env_hub = install.path().display().to_string();
    let flag_hub = flag_root.path().display().to_string();
    // 两级同时给出：--hub 生效，FSH_HUB 指向的根一个字节不被触碰
    let r = json_env(&[("FSH_HUB", &env_hub)], &flag_hub, &["init"]);
    assert_eq!(r.code, 0, "init --hub 优先: {}", r.stdout);
    assert!(flag_root.path().join("store").is_dir());
    assert!(
        !install.path().join("store").exists(),
        "FSH_HUB 的根不应被使用"
    );
}
