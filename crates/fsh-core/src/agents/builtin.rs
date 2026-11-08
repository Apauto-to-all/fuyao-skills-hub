//! 内置 agent 名单：随程序发布的静态登记表。
//! 用户在 hub 内放同名 TOML 即整项覆盖，见 merge 子模块。

/// 内置表一行：与用户 TOML 同一条规则——两个技能位都可选，至少给一个。
/// 键直接作为 `--agent` 取值；项目目录不做命名推断，按名单原样记录；
/// 全局目录写带 `~` 的可移植字面量，表里不出现任何绝对路径。
pub(super) struct BuiltinAgent {
    /// `--agent` 取值
    pub(super) key: &'static str,
    /// 给人看的名字
    pub(super) display_name: &'static str,
    /// 项目内相对技能目录；没有项目位是 None
    pub(super) skills_dir: Option<&'static str>,
    /// 可移植写法的全局技能目录；没有全局位是 None
    pub(super) global_skills_dir: Option<&'static str>,
}

impl BuiltinAgent {
    /// 两个技能位都有
    const fn both(
        key: &'static str,
        display_name: &'static str,
        skills_dir: &'static str,
        global_skills_dir: &'static str,
    ) -> Self {
        Self {
            key,
            display_name,
            skills_dir: Some(skills_dir),
            global_skills_dir: Some(global_skills_dir),
        }
    }

    /// 只有项目位
    const fn project_only(
        key: &'static str,
        display_name: &'static str,
        skills_dir: &'static str,
    ) -> Self {
        Self {
            key,
            display_name,
            skills_dir: Some(skills_dir),
            global_skills_dir: None,
        }
    }
}

pub(super) const BUILTIN: &[BuiltinAgent] = &[
    // 通用位：不属于任何具体厂商，所有 agent 共用的目录约定
    BuiltinAgent::both("universal", "通用", ".agents/skills", "~/.agents/skills"),
    // 国产 agent：厂商均在中国内地
    BuiltinAgent::both(
        "autoclaw",
        "AutoClaw",
        ".openclaw-autoclaw/skills",
        "~/.openclaw-autoclaw/skills",
    ),
    BuiltinAgent::both(
        "codearts-agent",
        "CodeArts Agent",
        ".codeartsdoer/skills",
        "~/.codeartsdoer/skills",
    ),
    BuiltinAgent::both(
        "codebuddy",
        "CodeBuddy",
        ".codebuddy/skills",
        "~/.codebuddy/skills",
    ),
    BuiltinAgent::both("dsh", "DeepSeek Harness", ".dsh/skills", "~/.dsh/skills"),
    BuiltinAgent::both("iflow-cli", "iFlow CLI", ".iflow/skills", "~/.iflow/skills"),
    BuiltinAgent::both(
        "kimi-code-cli",
        "Kimi Code CLI",
        ".agents/skills",
        "~/.agents/skills",
    ),
    BuiltinAgent::both("lingma", "Lingma", ".lingma/skills", "~/.lingma/skills"),
    BuiltinAgent::both(
        "minimax-code",
        "MiniMax Code",
        ".minimax/skills",
        "~/.minimax/skills",
    ),
    BuiltinAgent::both("qoder", "Qoder", ".qoder/skills", "~/.qoder/skills"),
    BuiltinAgent::both(
        "qoder-cn",
        "Qoder CN",
        ".qoder/skills",
        "~/.qoder-cn/skills",
    ),
    BuiltinAgent::both("qwen-code", "Qwen Code", ".qwen/skills", "~/.qwen/skills"),
    BuiltinAgent::both("trae", "Trae", ".trae/skills", "~/.trae/skills"),
    BuiltinAgent::both("trae-cn", "Trae CN", ".trae/skills", "~/.trae-cn/skills"),
    BuiltinAgent::both(
        "workbuddy",
        "WorkBuddy",
        ".workbuddy/skills",
        "~/.workbuddy/skills",
    ),
    BuiltinAgent::both(
        "workbuddy-overseas",
        "WorkBuddy AI",
        ".workbuddy-ai/skills",
        "~/.workbuddy-ai/skills",
    ),
    BuiltinAgent::both("zcode", "ZCode", ".zcode/skills", "~/.zcode/skills"),
    // 国外 agent：其余厂商
    BuiltinAgent::both("adal", "AdaL", ".adal/skills", "~/.adal/skills"),
    BuiltinAgent::both(
        "aider-desk",
        "AiderDesk",
        ".aider-desk/skills",
        "~/.aider-desk/skills",
    ),
    BuiltinAgent::both("amp", "Amp", ".agents/skills", "~/.config/agents/skills"),
    BuiltinAgent::both(
        "antigravity",
        "Antigravity",
        ".agents/skills",
        "~/.gemini/config/skills",
    ),
    BuiltinAgent::both(
        "antigravity-cli",
        "Antigravity CLI",
        ".agents/skills",
        "~/.gemini/config/skills",
    ),
    BuiltinAgent::both(
        "astrbot",
        "AstrBot",
        "data/skills",
        "~/.astrbot/data/skills",
    ),
    BuiltinAgent::both("augment", "Augment", ".augment/skills", "~/.augment/skills"),
    BuiltinAgent::both(
        "autohand-code",
        "Autohand Code CLI",
        ".autohand/skills",
        "~/.autohand/skills",
    ),
    BuiltinAgent::both("bob", "IBM Bob", ".bob/skills", "~/.bob/skills"),
    BuiltinAgent::both(
        "claude-code",
        "Claude Code",
        ".claude/skills",
        "~/.claude/skills",
    ),
    BuiltinAgent::both("cline", "Cline", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both("codex", "Codex", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both(
        "codemaker",
        "Codemaker",
        ".codemaker/skills",
        "~/.codemaker/skills",
    ),
    BuiltinAgent::both(
        "codestudio",
        "Code Studio",
        ".codestudio/skills",
        "~/.codestudio/skills",
    ),
    BuiltinAgent::both(
        "command-code",
        "Command Code",
        ".commandcode/skills",
        "~/.commandcode/skills",
    ),
    BuiltinAgent::both(
        "continue",
        "Continue",
        ".continue/skills",
        "~/.continue/skills",
    ),
    BuiltinAgent::both(
        "cortex",
        "Cortex Code",
        ".cortex/skills",
        "~/.snowflake/cortex/skills",
    ),
    BuiltinAgent::both("crush", "Crush", ".crush/skills", "~/.config/crush/skills"),
    BuiltinAgent::both("cursor", "Cursor", ".agents/skills", "~/.cursor/skills"),
    BuiltinAgent::both(
        "deepagents",
        "Deep Agents",
        ".agents/skills",
        "~/.deepagents/agent/skills",
    ),
    BuiltinAgent::both(
        "devin",
        "Devin for Terminal",
        ".devin/skills",
        "~/.config/devin/skills",
    ),
    BuiltinAgent::both("dexto", "Dexto", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both("droid", "Droid", ".agents/skills", "~/.factory/skills"),
    BuiltinAgent::project_only("eve", "Eve", "agent/skills"),
    BuiltinAgent::both(
        "firebender",
        "Firebender",
        ".agents/skills",
        "~/.firebender/skills",
    ),
    BuiltinAgent::both("forgecode", "ForgeCode", ".forge/skills", "~/.forge/skills"),
    BuiltinAgent::both("fx", "fx", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both(
        "gemini-cli",
        "Gemini CLI",
        ".agents/skills",
        "~/.gemini/skills",
    ),
    BuiltinAgent::both(
        "github-copilot",
        "GitHub Copilot",
        ".agents/skills",
        "~/.copilot/skills",
    ),
    BuiltinAgent::both("goose", "Goose", ".goose/skills", "~/.config/goose/skills"),
    BuiltinAgent::both("grok", "Grok Build", ".grok/skills", "~/.grok/skills"),
    BuiltinAgent::both(
        "hermes-agent",
        "Hermes Agent",
        ".hermes/skills",
        "~/.hermes/skills",
    ),
    BuiltinAgent::both(
        "inference-sh",
        "inference.sh",
        ".inferencesh/skills",
        "~/.inferencesh/skills",
    ),
    BuiltinAgent::both("jazz", "Jazz", ".jazz/skills", "~/.jazz/skills"),
    BuiltinAgent::both("junie", "Junie", ".junie/skills", "~/.junie/skills"),
    BuiltinAgent::both("kilo", "Kilo Code", ".agents/skills", "~/.kilo/skills"),
    BuiltinAgent::both(
        "kimchi",
        "Kimchi",
        ".kimchi/skills",
        "~/.config/kimchi/harness/skills",
    ),
    BuiltinAgent::both("kiro-cli", "Kiro CLI", ".kiro/skills", "~/.kiro/skills"),
    BuiltinAgent::both("kode", "Kode", ".kode/skills", "~/.kode/skills"),
    BuiltinAgent::both("loaf", "Loaf", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both("mcpjam", "MCPJam", ".mcpjam/skills", "~/.mcpjam/skills"),
    BuiltinAgent::both(
        "mistral-vibe",
        "Mistral Vibe",
        ".vibe/skills",
        "~/.vibe/skills",
    ),
    BuiltinAgent::both("moxby", "Moxby", ".moxby/skills", "~/.moxby/skills"),
    BuiltinAgent::both("mux", "Mux", ".mux/skills", "~/.mux/skills"),
    BuiltinAgent::both("neovate", "Neovate", ".neovate/skills", "~/.neovate/skills"),
    BuiltinAgent::both("ona", "Ona", ".ona/skills", "~/.ona/skills"),
    BuiltinAgent::both("openclaw", "OpenClaw", "skills", "~/.openclaw/skills"),
    BuiltinAgent::both(
        "opencode",
        "OpenCode",
        ".agents/skills",
        "~/.config/opencode/skills",
    ),
    BuiltinAgent::both(
        "openhands",
        "OpenHands",
        ".openhands/skills",
        "~/.openhands/skills",
    ),
    BuiltinAgent::both("pi", "Pi", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both(
        "posit-assistant",
        "Posit Assistant",
        ".posit/assistant/skills",
        "~/.posit/assistant/skills",
    ),
    BuiltinAgent::both("pochi", "Pochi", ".pochi/skills", "~/.pochi/skills"),
    BuiltinAgent::project_only("promptscript", "PromptScript", ".agents/skills"),
    BuiltinAgent::both(
        "reasonix",
        "Reasonix",
        ".reasonix/skills",
        "~/.reasonix/skills",
    ),
    BuiltinAgent::both(
        "replit",
        "Replit",
        ".agents/skills",
        "~/.config/agents/skills",
    ),
    BuiltinAgent::both("roo", "Roo Code", ".roo/skills", "~/.roo/skills"),
    BuiltinAgent::both(
        "rovodev",
        "Rovo Dev",
        ".rovodev/skills",
        "~/.rovodev/skills",
    ),
    BuiltinAgent::both(
        "sarvam-code",
        "Sarvam Code",
        ".agents/skills",
        "~/.agents/skills",
    ),
    BuiltinAgent::both(
        "tabnine-cli",
        "Tabnine CLI",
        ".tabnine/agent/skills",
        "~/.tabnine/agent/skills",
    ),
    BuiltinAgent::both(
        "terramind",
        "Terramind",
        ".terramind/skills",
        "~/.terramind/skills",
    ),
    BuiltinAgent::both(
        "tinycloud",
        "Tinycloud",
        ".tinycloud/skills",
        "~/.tinycloud/skills",
    ),
    BuiltinAgent::both("warp", "Warp", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both(
        "windsurf",
        "Windsurf",
        ".windsurf/skills",
        "~/.codeium/windsurf/skills",
    ),
    BuiltinAgent::both("zed", "Zed", ".agents/skills", "~/.agents/skills"),
    BuiltinAgent::both(
        "zencoder",
        "Zencoder",
        ".zencoder/skills",
        "~/.zencoder/skills",
    ),
    BuiltinAgent::both(
        "zenflow",
        "Zenflow",
        ".zencoder/skills",
        "~/.zencoder/skills",
    ),
];
