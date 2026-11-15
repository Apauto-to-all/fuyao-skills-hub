//! 全局根适配的解析测试：家目录、变量取值、平台标志与存在性探测
//! 全部由测试注入，不碰真实环境；回落一律断言到表里默认字面量的展开结果。

use std::fs;
use std::path::PathBuf;

use crate::agents::builtin::BUILTIN;
use crate::agents::global_root::{GlobalRoot, resolve};

/// 变量表夹具：按名字查值，查不到即未设置
fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.to_string())
    }
}

/// 家目录夹具：临时目录当 home，守卫持有到测试结束
fn home() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    (dir, path)
}

/// 专有变量五元组名单：变量名 / 表字面量 / 变量设置时取值
const PROPRIETARY: &[(&str, &str, &str)] = &[
    ("CLAUDE_CONFIG_DIR", "~/.claude/skills", "D:/custom/claude"),
    ("GROK_HOME", "~/.grok/skills", "D:/custom/grok"),
    ("HERMES_HOME", "~/.hermes/skills", "D:/custom/hermes"),
    ("VIBE_HOME", "~/.vibe/skills", "D:/custom/vibe"),
    ("AUTOHAND_HOME", "~/.autohand/skills", "D:/custom/autohand"),
];

/// 专有变量两态：设置且非空 → 变量值为根拼子路径；未设置 → 回落表字面量
#[test]
fn proprietary_var_set_uses_var_root_and_unset_falls_back() {
    let (_guard, home) = home();
    for (name, literal, value) in PROPRIETARY {
        let root = GlobalRoot::Var {
            name,
            sub: "skills",
        };
        let set = resolve(root, literal, &home, false, &vars(&[(name, value)])).unwrap();
        assert_eq!(
            set,
            PathBuf::from(value).join("skills"),
            "{name} 设置时应取变量根"
        );
        let unset = resolve(root, literal, &home, false, &vars(&[])).unwrap();
        assert_eq!(
            unset,
            home.join(literal.trim_start_matches("~/")),
            "{name} 未设置时应回落表字面量"
        );
    }
}

/// 空值与相对取值都视同未设置：落点必须是绝对路径，宁回落也不产出相对落点
#[test]
fn blank_or_relative_var_value_falls_back() {
    let (_guard, home) = home();
    let root = GlobalRoot::Var {
        name: "CLAUDE_CONFIG_DIR",
        sub: "skills",
    };
    for value in ["", "  "] {
        let got = resolve(
            root,
            "~/.claude/skills",
            &home,
            false,
            &vars(&[("CLAUDE_CONFIG_DIR", value)]),
        )
        .unwrap();
        assert_eq!(got, home.join(".claude/skills"), "空值 {value:?} 应回落");
    }
    let got = resolve(
        root,
        "~/.claude/skills",
        &home,
        false,
        &vars(&[("CLAUDE_CONFIG_DIR", "relative/dir")]),
    )
    .unwrap();
    assert_eq!(got, home.join(".claude/skills"), "相对取值应回落");
}

/// XDG 根五条目的两态：设置 → 变量根拼子路径；未设置 → 回落各自表字面量。
/// universal 的回落字面量与 XDG 子路径不同根，两态各自独立
#[test]
fn xdg_root_set_and_unset_states() {
    let (_guard, home) = home();
    let entries: &[(GlobalRoot, &str)] = &[
        (
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "opencode/skills",
            },
            "~/.config/opencode/skills",
        ),
        (
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "goose/skills",
            },
            "~/.config/goose/skills",
        ),
        (
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "devin/skills",
            },
            "~/.config/devin/skills",
        ),
        // universal：回落字面量与子路径不同根，两态各自断言
        (
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "agents/skills",
            },
            "~/.agents/skills",
        ),
    ];
    for (root, literal) in entries {
        let set = resolve(
            *root,
            literal,
            &home,
            false,
            &vars(&[("XDG_CONFIG_HOME", "D:/xdg")]),
        )
        .unwrap();
        let sub = match root {
            GlobalRoot::Var { sub, .. } => *sub,
            _ => unreachable!("名单里只有 XDG 条目"),
        };
        assert_eq!(set, PathBuf::from("D:/xdg").join(sub));
        let unset = resolve(*root, literal, &home, false, &vars(&[])).unwrap();
        assert_eq!(
            unset,
            home.join(literal.trim_start_matches("~/")),
            "回落必须是表字面量 {literal}"
        );
    }
}

/// reasonix 平台-变量组合：变量优先；Windows 回落 APPDATA，再回落家目录
/// Roaming 路径；非 Windows 回落 `~/.reasonix`
#[test]
fn reasonix_platform_and_var_combinations() {
    let (_guard, home) = home();
    let root = GlobalRoot::VarWin {
        name: "REASONIX_HOME",
        win_dir: "reasonix",
        sub: "skills",
    };
    // 变量设置：平台不影响
    for windows in [false, true] {
        let got = resolve(
            root,
            "~/.reasonix/skills",
            &home,
            windows,
            &vars(&[("REASONIX_HOME", "D:/rx")]),
        )
        .unwrap();
        assert_eq!(got, PathBuf::from("D:/rx").join("skills"));
    }
    // Windows + APPDATA 设置
    let got = resolve(
        root,
        "~/.reasonix/skills",
        &home,
        true,
        &vars(&[("APPDATA", "D:/appdata")]),
    )
    .unwrap();
    assert_eq!(got, PathBuf::from("D:/appdata").join("reasonix/skills"));
    // Windows + APPDATA 未设置：家目录 Roaming 路径
    let got = resolve(root, "~/.reasonix/skills", &home, true, &vars(&[])).unwrap();
    assert_eq!(got, home.join("AppData/Roaming/reasonix/skills"));
    // 非 Windows：家目录 .reasonix
    let got = resolve(root, "~/.reasonix/skills", &home, false, &vars(&[])).unwrap();
    assert_eq!(got, home.join(".reasonix/skills"));
}

/// openclaw 存在性探测四态：三候选各一 + 全不存在用第一个候选
#[test]
fn openclaw_probe_four_states() {
    let (guard, home) = home();
    let root = GlobalRoot::Probe {
        candidates: &[".openclaw", ".clawdbot", ".moltbot"],
        sub: "skills",
    };
    let pick = |dir: &str| home.join(dir).join("skills");
    // 全不存在 → 第一个候选
    assert_eq!(
        resolve(root, "~/.openclaw/skills", &home, false, &vars(&[])).unwrap(),
        pick(".openclaw")
    );
    // 只有第二个候选存在
    fs::create_dir_all(home.join(".clawdbot")).unwrap();
    assert_eq!(
        resolve(root, "~/.openclaw/skills", &home, false, &vars(&[])).unwrap(),
        pick(".clawdbot")
    );
    // 第一个候选出现后优先
    fs::create_dir_all(home.join(".moltbot")).unwrap();
    fs::create_dir_all(home.join(".openclaw")).unwrap();
    assert_eq!(
        resolve(root, "~/.openclaw/skills", &home, false, &vars(&[])).unwrap(),
        pick(".openclaw")
    );
    drop(guard);
}

/// 只有第三个候选存在：仍按声明顺序取第一个存在的
#[test]
fn openclaw_probe_picks_third_when_sole() {
    let (_guard, home) = home();
    fs::create_dir_all(home.join(".moltbot")).unwrap();
    assert_eq!(
        resolve(
            GlobalRoot::Probe {
                candidates: &[".openclaw", ".clawdbot", ".moltbot"],
                sub: "skills",
            },
            "~/.openclaw/skills",
            &home,
            false,
            &vars(&[]),
        )
        .unwrap(),
        home.join(".moltbot/skills")
    );
}

/// 内置表十三条适配条目必须携带声明的根；其余条目不带声明
#[test]
fn builtin_table_carries_adaptation_declarations() {
    let expected: &[(&str, GlobalRoot)] = &[
        (
            "claude-code",
            GlobalRoot::Var {
                name: "CLAUDE_CONFIG_DIR",
                sub: "skills",
            },
        ),
        (
            "grok",
            GlobalRoot::Var {
                name: "GROK_HOME",
                sub: "skills",
            },
        ),
        (
            "hermes-agent",
            GlobalRoot::Var {
                name: "HERMES_HOME",
                sub: "skills",
            },
        ),
        (
            "mistral-vibe",
            GlobalRoot::Var {
                name: "VIBE_HOME",
                sub: "skills",
            },
        ),
        (
            "autohand-code",
            GlobalRoot::Var {
                name: "AUTOHAND_HOME",
                sub: "skills",
            },
        ),
        (
            "reasonix",
            GlobalRoot::VarWin {
                name: "REASONIX_HOME",
                win_dir: "reasonix",
                sub: "skills",
            },
        ),
        (
            "amp",
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "agents/skills",
            },
        ),
        (
            "universal",
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "agents/skills",
            },
        ),
        (
            "replit",
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "agents/skills",
            },
        ),
        (
            "opencode",
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "opencode/skills",
            },
        ),
        (
            "goose",
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "goose/skills",
            },
        ),
        (
            "devin",
            GlobalRoot::Var {
                name: "XDG_CONFIG_HOME",
                sub: "devin/skills",
            },
        ),
        (
            "openclaw",
            GlobalRoot::Probe {
                candidates: &[".openclaw", ".clawdbot", ".moltbot"],
                sub: "skills",
            },
        ),
    ];
    for (key, root) in expected {
        let row = BUILTIN.iter().find(|r| r.key == *key).unwrap();
        assert_eq!(row.global_root, Some(*root), "{key} 的适配声明不对");
    }
    let adapted: Vec<&str> = expected.iter().map(|(k, _)| *k).collect();
    for row in BUILTIN {
        if !adapted.contains(&row.key) {
            assert_eq!(row.global_root, None, "{} 不该带适配声明", row.key);
        }
    }
}
