# 内置 agent 配置表

下表就是内置名单的发布形态：`--agent` 取值必须是这张表里的键，或 hub 里 `agents\<键>.toml` 新增/覆盖出来的键。
全局技能目录一列写的是可移植字面量（`~` 与变量在解析落点时展开，落点永远指向绝对路径）；`全局根适配` 一列说明该条目还会先看专有变量或探测候选目录——命中就以它作根再拼接行尾的子路径，没命中才回落左边的字面量。
项目技能目录写 `—` 的条目只有全局位，只能用 `--global`（含用户覆盖项）；全局技能目录写 `—` 的条目只有项目位，只能用 `--project`。

| 键（--agent） | 显示名 | 项目技能目录 | 全局技能目录 | 全局根适配 |
| --- | --- | --- | --- | --- |
| `adal` | AdaL | `.adal/skills` | `~/.adal/skills` | — |
| `aider-desk` | AiderDesk | `.aider-desk/skills` | `~/.aider-desk/skills` | — |
| `amp` | Amp | `.agents/skills` | `~/.config/agents/skills` | 设了 XDG_CONFIG_HOME 就以它作根，拼接 agents/skills |
| `antigravity-cli` | Antigravity CLI | `.agents/skills` | `~/.gemini/config/skills` | — |
| `antigravity` | Antigravity | `.agents/skills` | `~/.gemini/config/skills` | — |
| `astrbot` | AstrBot | `data/skills` | `~/.astrbot/data/skills` | — |
| `augment` | Augment | `.augment/skills` | `~/.augment/skills` | — |
| `autoclaw` | AutoClaw | `.openclaw-autoclaw/skills` | `~/.openclaw-autoclaw/skills` | — |
| `autohand-code` | Autohand Code CLI | `.autohand/skills` | `~/.autohand/skills` | 设了 AUTOHAND_HOME 就以它作根，拼接 skills |
| `bob` | IBM Bob | `.bob/skills` | `~/.bob/skills` | — |
| `claude-code` | Claude Code | `.claude/skills` | `~/.claude/skills` | 设了 CLAUDE_CONFIG_DIR 就以它作根，拼接 skills |
| `cline` | Cline | `.agents/skills` | `~/.agents/skills` | — |
| `codearts-agent` | CodeArts Agent | `.codeartsdoer/skills` | `~/.codeartsdoer/skills` | — |
| `codebuddy` | CodeBuddy | `.codebuddy/skills` | `~/.codebuddy/skills` | — |
| `codemaker` | Codemaker | `.codemaker/skills` | `~/.codemaker/skills` | — |
| `codestudio` | Code Studio | `.codestudio/skills` | `~/.codestudio/skills` | — |
| `codex` | Codex | `.agents/skills` | `~/.agents/skills` | — |
| `command-code` | Command Code | `.commandcode/skills` | `~/.commandcode/skills` | — |
| `continue` | Continue | `.continue/skills` | `~/.continue/skills` | — |
| `cortex` | Cortex Code | `.cortex/skills` | `~/.snowflake/cortex/skills` | — |
| `crush` | Crush | `.crush/skills` | `~/.config/crush/skills` | — |
| `cursor` | Cursor | `.agents/skills` | `~/.cursor/skills` | — |
| `deepagents` | Deep Agents | `.agents/skills` | `~/.deepagents/agent/skills` | — |
| `devin` | Devin for Terminal | `.devin/skills` | `~/.config/devin/skills` | 设了 XDG_CONFIG_HOME 就以它作根，拼接 devin/skills |
| `dexto` | Dexto | `.agents/skills` | `~/.agents/skills` | — |
| `droid` | Droid | `.agents/skills` | `~/.factory/skills` | — |
| `dsh` | DeepSeek Harness | `.dsh/skills` | `~/.dsh/skills` | — |
| `eve` | Eve | `agent/skills` | `—` | — |
| `firebender` | Firebender | `.agents/skills` | `~/.firebender/skills` | — |
| `forgecode` | ForgeCode | `.forge/skills` | `~/.forge/skills` | — |
| `fx` | fx | `.agents/skills` | `~/.agents/skills` | — |
| `gemini-cli` | Gemini CLI | `.agents/skills` | `~/.gemini/skills` | — |
| `github-copilot` | GitHub Copilot | `.agents/skills` | `~/.copilot/skills` | — |
| `goose` | Goose | `.goose/skills` | `~/.config/goose/skills` | 设了 XDG_CONFIG_HOME 就以它作根，拼接 goose/skills |
| `grok` | Grok Build | `.grok/skills` | `~/.grok/skills` | 设了 GROK_HOME 就以它作根，拼接 skills |
| `hermes-agent` | Hermes Agent | `.hermes/skills` | `~/.hermes/skills` | 设了 HERMES_HOME 就以它作根，拼接 skills |
| `iflow-cli` | iFlow CLI | `.iflow/skills` | `~/.iflow/skills` | — |
| `inference-sh` | inference.sh | `.inferencesh/skills` | `~/.inferencesh/skills` | — |
| `jazz` | Jazz | `.jazz/skills` | `~/.jazz/skills` | — |
| `junie` | Junie | `.junie/skills` | `~/.junie/skills` | — |
| `kilo` | Kilo Code | `.agents/skills` | `~/.kilo/skills` | — |
| `kimchi` | Kimchi | `.kimchi/skills` | `~/.config/kimchi/harness/skills` | — |
| `kimi-code-cli` | Kimi Code CLI | `.agents/skills` | `~/.agents/skills` | — |
| `kiro-cli` | Kiro CLI | `.kiro/skills` | `~/.kiro/skills` | — |
| `kode` | Kode | `.kode/skills` | `~/.kode/skills` | — |
| `lingma` | Lingma | `.lingma/skills` | `~/.lingma/skills` | — |
| `loaf` | Loaf | `.agents/skills` | `~/.agents/skills` | — |
| `mcpjam` | MCPJam | `.mcpjam/skills` | `~/.mcpjam/skills` | — |
| `minimax-code` | MiniMax Code | `.minimax/skills` | `~/.minimax/skills` | — |
| `mistral-vibe` | Mistral Vibe | `.vibe/skills` | `~/.vibe/skills` | 设了 VIBE_HOME 就以它作根，拼接 skills |
| `moxby` | Moxby | `.moxby/skills` | `~/.moxby/skills` | — |
| `mux` | Mux | `.mux/skills` | `~/.mux/skills` | — |
| `neovate` | Neovate | `.neovate/skills` | `~/.neovate/skills` | — |
| `ona` | Ona | `.ona/skills` | `~/.ona/skills` | — |
| `openclaw` | OpenClaw | `skills` | `~/.openclaw/skills` | 按序探测家目录下的 .openclaw、.clawdbot、.moltbot，取第一个存在的作根，拼接 skills |
| `opencode` | OpenCode | `.agents/skills` | `~/.config/opencode/skills` | 设了 XDG_CONFIG_HOME 就以它作根，拼接 opencode/skills |
| `openhands` | OpenHands | `.openhands/skills` | `~/.openhands/skills` | — |
| `pi` | Pi | `.agents/skills` | `~/.agents/skills` | — |
| `pochi` | Pochi | `.pochi/skills` | `~/.pochi/skills` | — |
| `posit-assistant` | Posit Assistant | `.posit/assistant/skills` | `~/.posit/assistant/skills` | — |
| `promptscript` | PromptScript | `.agents/skills` | `—` | — |
| `qoder-cn` | Qoder CN | `.qoder/skills` | `~/.qoder-cn/skills` | — |
| `qoder` | Qoder | `.qoder/skills` | `~/.qoder/skills` | — |
| `qwen-code` | Qwen Code | `.qwen/skills` | `~/.qwen/skills` | — |
| `reasonix` | Reasonix | `.reasonix/skills` | `~/.reasonix/skills` | 设了 REASONIX_HOME 就以它作根；Windows 上没设时回落 APPDATA/reasonix，拼接 skills |
| `replit` | Replit | `.agents/skills` | `~/.config/agents/skills` | 设了 XDG_CONFIG_HOME 就以它作根，拼接 agents/skills |
| `roo` | Roo Code | `.roo/skills` | `~/.roo/skills` | — |
| `rovodev` | Rovo Dev | `.rovodev/skills` | `~/.rovodev/skills` | — |
| `sarvam-code` | Sarvam Code | `.agents/skills` | `~/.agents/skills` | — |
| `tabnine-cli` | Tabnine CLI | `.tabnine/agent/skills` | `~/.tabnine/agent/skills` | — |
| `terramind` | Terramind | `.terramind/skills` | `~/.terramind/skills` | — |
| `tinycloud` | Tinycloud | `.tinycloud/skills` | `~/.tinycloud/skills` | — |
| `trae-cn` | Trae CN | `.trae/skills` | `~/.trae-cn/skills` | — |
| `trae` | Trae | `.trae/skills` | `~/.trae/skills` | — |
| `universal` | 通用 | `.agents/skills` | `~/.agents/skills` | 设了 XDG_CONFIG_HOME 就以它作根，拼接 agents/skills |
| `warp` | Warp | `.agents/skills` | `~/.agents/skills` | — |
| `windsurf` | Windsurf | `.windsurf/skills` | `~/.codeium/windsurf/skills` | — |
| `workbuddy-overseas` | WorkBuddy AI | `.workbuddy-ai/skills` | `~/.workbuddy-ai/skills` | — |
| `workbuddy` | WorkBuddy | `.workbuddy/skills` | `~/.workbuddy/skills` | — |
| `zcode` | ZCode | `.zcode/skills` | `~/.zcode/skills` | — |
| `zed` | Zed | `.agents/skills` | `~/.agents/skills` | — |
| `zencoder` | Zencoder | `.zencoder/skills` | `~/.zencoder/skills` | — |
| `zenflow` | Zenflow | `.zencoder/skills` | `~/.zencoder/skills` | — |

