# SKILL.md frontmatter：官方字段全集、扩展位与各工具对未知字段的态度

- 调研日期：2026-11-08
- 对应：P4「frontmatter 字段合理化」
- 范围：`SKILL.md` 顶层 frontmatter 的官方字段全集与约束；官方扩展位与嵌套/值类型限制；本仓 `crates/fsh-core/src/agents/builtin.rs` 内置表中常见工具读 frontmatter 时对未知顶层字段的处理；`version` / `source` / `tags` 三类信息在各家既有字段里有无语义等价物。
- 来源：agentskills.io 规范与其官方参考实现 `skills-ref` 源码、Anthropic 官方文档（platform / Claude Code）、各工具官方文档或官方仓库文档源码。查不到的一律标「未证实」。下文「事实」与推断分开，不给兼容或迁移方案。
- 事实基准日：2026-11-08 抓取的页面。这些页面无版本号，只有抓取当日的内容。

## 结论速览

1. `SKILL.md` 的事实标准是 agentskills.io 规范，顶层只有 6 个字段：`name`、`description` 必填，`license`、`compatibility`、`metadata`、`allowed-tools` 选填；没有 `version`、`tags`、`source`。  
   来源：https://agentskills.io/specification
2. `name` 限 1–64 字符、小写字母数字加连字符、首尾不得是连字符、不得出现连续连字符，且**必须等于父目录名**；`description` 限 1–1024 字符。  
   来源：https://agentskills.io/specification#name-field
3. Anthropic 在 claude.ai / Skills API 路径上对 `name` 另有禁令：`name` 与 `description` 都不得含 XML 标签，`name` 不得含保留词 `anthropic`、`claude`。  
   来源：https://docs.claude.com/en/agents-and-tools/agent-skills/overview
4. 官方扩展位就是 `metadata`：定位为「字符串键 → 字符串值」的映射，明确允许存规范未定义的属性，并建议键名足够独特以避免意外撞名。  
   来源：https://agentskills.io/specification#metadata-field
5. 官方参考校验器把这条落成硬约束：顶层出现 6 个字段之外的任何名字即报 `Unexpected fields in frontmatter`，且不校验 `metadata` 内部。  
   来源：https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py
6. Claude Code 自己**忽略**不认识的顶层字段且不报错；同一份文件走 claude.ai 上传 / Skills API / `package_skill.py` 时，同一批字段会变成硬错误 `Unexpected key(s) in SKILL.md frontmatter`。  
   来源：https://code.claude.com/docs/en/skills
7. 各家实现都在顶层塞私有字段：Claude Code 有十余个自有字段，Codex 把扩展放到旁路文件 `agents/openai.yaml`，opencode 把私有开关放进 `metadata.opencode/autoinvoke`，Amp 用顶层 `mcpServers`，Kimi 用顶层 `type: flow`。  
   来源：https://code.claude.com/docs/en/skills 、https://learn.chatgpt.com/docs/build-skills 、https://opencode.ai/docs/skills/ 、https://ampcode.com/docs/customize/skills 、https://github.com/moonshotai/kimi-cli/blob/main/docs/en/customization/skills.md
8. 跨工具真正全体共有的只有 `name` 与 `description`；`license` / `compatibility` / `metadata` 在规范层与 Claude Code、opencode、Kimi 上成立，其中 opencode 明说「接受但不解释」。  
   来源：https://opencode.ai/docs/skills/ 、https://code.claude.com/docs/en/skills
9. `version` 没有官方字段；规范自己的示例把版本写成 `metadata.version`，而真正需要版本的实现把它放在 `SKILL.md` 之外（opencode 的 catalog `index.json`、OpenAI API 的 `skill_reference.version`）。  
   来源：https://agentskills.io/specification#metadata-field 、https://opencode.ai/docs/skills/ 、https://developers.openai.com/api/docs/guides/tools-skills
10. `source` 与 `tags` 在官方字段里都没有等价物：`license` 是许可证不是出处，各工具的「来源」是安装侧记录而非 frontmatter。  
    来源：https://agentskills.io/specification 、https://code.visualstudio.com/docs/agent-customization/agent-skills

## 一、官方规范字段全集

agentskills.io 规范的原表只列 6 个字段，`name` / `description` 必填。下表是规范条目加上官方参考实现的落地细节。

| 字段 | 必填性 | 类型与约束 | 语义 | 来源 URL |
| --- | --- | --- | --- | --- |
| `name` | 必填 | 字符串；1–64 字符；仅小写字母、数字、连字符；首尾不得为连字符；不得出现 `--`；**必须等于父目录名** | 技能标识（也是各工具命令名 / 查找键的主要来源） | https://agentskills.io/specification#name-field |
| `description` | 必填 | 字符串；1–1024 字符；非空 | 做什么 + 何时使用；Claude 侧用它匹配触发 | https://agentskills.io/specification#description-field |
| `license` | 选填 | 字符串；规范只「建议简短」：许可证名，或随包许可证文件名 | 许可证 | https://agentskills.io/specification#license-field |
| `compatibility` | 选填 | 字符串；1–500 字符 | 环境要求（目标产品、系统包、网络等） | https://agentskills.io/specification#compatibility-field |
| `metadata` | 选填 | map：字符串键 → 字符串值 | 承载规范未定义的附加属性；官方扩展位 | https://agentskills.io/specification#metadata-field |
| `allowed-tools` | 选填（标注 Experimental） | 空格分隔的工具名字符串 | 预批准工具；规范明说「支持程度因实现而异」 | https://agentskills.io/specification#allowed-tools-field |

**参考实现的落地细节（事实）**

- `skills-ref` 的白名单是硬编码的 6 项，其余一律拒绝：`ALLOWED_FIELDS = {name, description, license, allowed-tools, metadata, compatibility}`；多出的字段报 `Unexpected fields in frontmatter: ... Only [...] are allowed.`  
  出处：https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py
- 同一个文件里 `name` 的校验比规范正文宽：代码允许 Unicode 字母（docstring 写「支持 i18n 字符」），而规范正文写的是 `a-z`、`0-9`。两处不一致，规范正文是文档口径、校验器是代码口径。  
  出处：https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py
- 校验器**不检查** `license` 的类型，也**不检查** `metadata` 内部的键值类型与嵌套深度；`metadata` 的「字符串 → 字符串」只是规范文字。  
  出处：https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py
- Anthropic 平台文档在 6 字段之外另有 `name` / `description` 的额外禁令：不得含 XML 标签；`name` 不得含保留词 `anthropic`、`claude`。  
  出处：https://docs.claude.com/en/agents-and-tools/agent-skills/overview
- Claude Code 的 frontmatter 表比规范大得多，共 20 个字段：`name`、`description`、`when_to_use`、`argument-hint`、`arguments`、`disable-model-invocation`、`user-invocable`、`allowed-tools`、`disallowed-tools`、`model`、`effort`、`context`、`agent`、`background`、`hooks`、`paths`、`shell`、`metadata`、`license`、`compatibility`；且明说「全部字段可选，只有 `description` 是推荐」。  
  出处：https://code.claude.com/docs/en/skills
- Claude Code 的 `description` 语义与规范不同：它与 `when_to_use` 合并后在技能列表里被截到 1,536 字符；缺省时取正文第一行非空内容。  
  出处：https://code.claude.com/docs/en/skills

## 二、扩展位与未知字段行为

### 1. 官方扩展位 `metadata`

**事实**

- 规范把 `metadata` 定义为「map from string keys to string values」，用途写明是「Clients can use this to store additional properties not defined by the Agent Skills spec」，并「建议键名足够独特以避免意外冲突」。规范自带的示例就是 `metadata: {author: example-org, version: "1.0"}`。  
  出处：https://agentskills.io/specification#metadata-field
- 就规范文字而言，`metadata` 的嵌套只有一层：键直接映射到字符串值。规范没有给 `metadata` 内部再定义次级语义。  
  出处：https://agentskills.io/specification#metadata-field
- Claude Code 对 `metadata` 的处理是「自由格式 YAML map，Claude Code 不解释其内容；值不是 map 时被丢弃」，并要求「不要把 `paths` 这类 frontmatter 字段名当 key 复用」。  
  出处：https://code.claude.com/docs/en/skills
- 已有实现把私有开关放进 `metadata` 且用了超出「字符串值」的形态：opencode 读 `metadata.opencode/autoinvoke: false`（斜杠命名空间的键 + 布尔值）。这是实现层的既有做法，不是规范授权。  
  出处：https://opencode.ai/docs/skills/
- Codex 走的是另一条路：不在 frontmatter 扩展，而是另放 `agents/openai.yaml`，里面是 `interface` / `policy` / `dependencies` 三段。  
  出处：https://learn.chatgpt.com/docs/build-skills
- Amp 则把 `mcpServers` 直接写成 `SKILL.md` 的顶层 frontmatter 字段（还有同目录 `mcp.json` 作为替代，两者并存时 `mcpServers` 优先）。  
  出处：https://ampcode.com/docs/customize/skills

### 2. 各工具对未知顶层字段的态度

「官方文档明说」= 该工具自己的文档写了行为；「代码」= 官方仓库实现代码；「未证实」= 没查到该行为的公开说明。

| 工具 | 读的字段（文档所见） | 未知顶层字段 | 依据 |
| --- | --- | --- | --- |
| Claude Code | 20 个字段（见上） | **忽略、不报错**：「A field name must match the table exactly… Claude Code ignores a field it doesn't recognize without reporting an error」 | 官方文档明说，https://code.claude.com/docs/en/skills |
| Claude Code 同源的 claude.ai 上传 / Skills API / `package_skill.py` | 仅 6 个规范字段 | **硬错误，整包失败**：`Unexpected key(s) in SKILL.md frontmatter: argument-hint. Allowed properties are: allowed-tools, compatibility, description, license, metadata, name` | 官方文档明说（含错误原文），https://code.claude.com/docs/en/skills |
| agentskills.io 参考校验器 `skills-ref` | 仅 6 个规范字段 | **报错**（校验工具返回值里带 `Unexpected fields in frontmatter`） | 官方仓库代码，https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py |
| Codex / ChatGPT | `name`、`description` 必填；其余扩展走 `agents/openai.yaml` | 未证实（文档只写「must include name and description」） | 官方文档，https://learn.chatgpt.com/docs/build-skills |
| opencode | `name`（仅显示名）、`description`、`metadata.opencode/autoinvoke`、`disable-model-invocation` | 未证实；但文档明说 V2「不强制」名称正则、1–64 长度、`name` 与目录一致、description 长度上限，且「接受 `license`、`compatibility` 这类可移植字段但不解释」 | 官方文档，https://opencode.ai/docs/skills/ |
| Cursor | `name`（必填，须等于父目录名）、`description`（必填）、`paths`、`disable-model-invocation`、`icon`、`color`、`metadata`；`globs` 作为旧字段仍被接受 | 未证实；文档只写了「`SKILL.md` 读不出 / frontmatter 不解析 / 缺 `description` 时，Cloud Agent 运行中给出警告」，技能仍可按名调用 | 官方文档，https://cursor.com/docs/skills |
| GitHub Copilot（VS Code） | `name`（必填，须等于父目录名，≤64）、`description`（必填，≤1024）、`argument-hint`、`user-invocable`、`disable-model-invocation`、`context`（实验）等 | 未证实；文档明说「名字含非法字符会导致技能**静默加载失败**」 | 官方文档，https://code.visualstudio.com/docs/agent-customization/agent-skills |
| Gemini CLI | `name`、`description`（二者缺一即被**静默跳过**）；`name` 取自 frontmatter 而非目录名，非法字符 `: \ / < > * ? " \|` 被替换成 `-` | 未证实 | 官方文档，https://geminicli.com/docs/cli/skills/ 、https://geminicli.com/docs/cli/tutorials/skills-getting-started/ |
| Kimi Code CLI | `name`（可选，缺省取目录名，1–64，小写+数字+连字符）、`description`（1–1024）、`license`、`compatibility`（≤500）、`metadata`；另有自有顶层字段 `type: flow` | 未证实 | 官方仓库文档源码，https://github.com/moonshotai/kimi-cli/blob/main/docs/en/customization/skills.md |
| Qwen Code | 官方示例中 `name`、`description` 之外还写了顶层 `priority: 10` | 未证实（该页正文抓取被截断，`priority` 的取值语义未取得） | 官方文档，https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/ |
| Goose | `name`、`description`（文档写「requires YAML frontmatter with `name` and `description`」） | 未证实 | 官方文档，https://goose-docs.ai/docs/guides/context-engineering/using-skills/ |
| Amp | `name`、`description`；额外顶层 `mcpServers`；`name` 必须与目录名一致 | 未证实 | 官方文档，https://ampcode.com/docs/customize/skills |
| Replit | 本次未取得字段级文档 | 未证实 | https://docs.replit.com/features/agent/skills |
| DSH / DeepSeek Harness | 本次未取得 | 未证实 | 本机安装目录未能读取（受限 Bash 运行时不可用），且未找到公开的 DSH frontmatter 文档 |

**已确证的两档行为**：一种是「忽略并继续」（Claude Code 本地加载、opencode、Cursor 的告警路线），一种是「报错或整包拒绝」（`skills-ref` 校验、claude.ai 上传 / Skills API / `package_skill.py`）。同一个技能作者可能同时面对这两档，因为同一份 `SKILL.md` 会走不同路径。

## 三、跨工具共有字段与身份来源

**事实**

- 全体共有：`name`、`description`。所有查到的工具都读这两个字段（Codex、Goose、Amp 等明写必填或需要）。
- 规范层 + 三个实现共有：`license`、`compatibility`、`metadata`。其中 Claude Code「接受但不作用于行为」，opencode「接受但不解释」，Kimi 在字段表里照样列出。  
  出处：https://code.claude.com/docs/en/skills 、https://opencode.ai/docs/skills/ 、https://github.com/moonshotai/kimi-cli/blob/main/docs/en/customization/skills.md
- `allowed-tools`：规范标为实验性并明说支持程度因实现而异；Claude Code 有同名同义字段。  
  出处：https://agentskills.io/specification#allowed-tools-field 、https://code.claude.com/docs/en/skills
- 身份来自哪一侧各家不同：
  - Claude Code：默认用目录名，`name` 设了就覆盖（`name` 是 `/` 菜单里的命令名）。
  - opencode：**ID 由文件路径决定**，frontmatter `name` 只是展示名。
  - Gemini CLI：**名字取自 frontmatter `name`**，与目录名无关。
  - Amp、Cursor、VS Code/Copilot：`name` 必须与目录名一致。
  - 出处：https://code.claude.com/docs/en/skills 、https://opencode.ai/docs/skills/ 、https://geminicli.com/docs/cli/tutorials/skills-getting-started/ 、https://ampcode.com/docs/customize/skills 、https://cursor.com/docs/skills 、https://code.visualstudio.com/docs/agent-customization/agent-skills
- 没有查到任何工具「完全不读 frontmatter、只认目录名」：opencode 最接近，但它的 `description` 仍从 frontmatter 取，且缺 `description` 的技能不会被广告给模型。

## 四、可复用 vs 必须进扩展位的判定

| 信息 | 有官方等价字段 | 判定 | 依据 |
| --- | --- | --- | --- |
| 版本 | 无官方字段；只有 `metadata.version` 这一官方示例写法 | 必须进扩展位（`metadata`） | 规范 6 字段里没有 `version`（https://agentskills.io/specification）；示例把它写成 `metadata.version: "1.0"`（https://agentskills.io/specification#metadata-field）；`skills-ref` 白名单也不含 `version`（https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py） |
| 版本（机器读的语义） | 无：没有任何工具把 `metadata.version` 当发版输入 | 只能自用，语义由 `fsh` 自己定义 | opencode 的版本在 catalog `index.json` 条目上（`{"name":…, "version": "3", …}`）用于刷新缓存，不在 `SKILL.md` 里（https://opencode.ai/docs/skills/）；OpenAI API 的版本在请求侧 `skill_reference.version`（https://developers.openai.com/api/docs/guides/tools-skills） |
| 来源 | 无 | 必须进扩展位（`metadata`） | 规范 6 字段无出处类字段（https://agentskills.io/specification）；`license` 是许可证，不是出处（https://agentskills.io/specification#license-field）；各工具的「来源」是安装侧记录而非 frontmatter，例如 VS Code 跟踪 MarketPlace 安装目录、Amp 把技能存进各自 git 仓库（https://code.visualstudio.com/docs/agent-customization/agent-skills 、https://ampcode.com/docs/customize/skills） |
| 标签 / 分类 | 无 | 必须进扩展位（`metadata`） | 规范无 `tags` / `keywords`；查到的工具字段表里也没有标签类字段（Claude Code https://code.claude.com/docs/en/skills ；Cursor 只有 UI 用的 `icon` / `color`，https://cursor.com/docs/skills ；Qwen 的 `priority` 是排序，不是分类，https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/） |

**同一张表要一起读的两条约束（事实，不是取舍）**

- `metadata` 的规范形态是「字符串 → 字符串」，所以标签这类多值信息只能编码成一个字符串放进去；把它写成 YAML 列表就超出规范文字（`skills-ref` 不检查这一点，Claude Code 也不检查内层，但规范文字如此）。  
  出处：https://agentskills.io/specification#metadata-field 、https://github.com/agentskills/agentskills/blob/main/skills-ref/src/skills_ref/validator.py
- 规范自己给出的「键名要足够独特以避免意外冲突」是它唯一的防撞名指引，没有规定命名空间前缀格式；`opencode/autoinvoke` 这种前缀写法出自实现，不是规范。  
  出处：https://agentskills.io/specification#metadata-field 、https://opencode.ai/docs/skills/

## 五、未证实与不确定项

- **Codex 对未知顶层字段的行为**：`learn.chatgpt.com/docs/build-skills` 与 `developers.openai.com/codex/skills/`（后者本次返回 403）都只写必填字段与旁路 `agents/openai.yaml`，没有说忽略还是拒绝。未证实。
- **opencode、Cursor、Copilot/VS Code、Gemini CLI、Kimi、Qwen、Goose、Amp 的未知字段处理**：这 8 家的公开文档都没有一句明确说明，本次只能从「文档列了哪些字段 + 是否有静默失败描述」间接判断。未证实，不宜按「忽略」推定。
- **Gemini CLI 的完整字段表**：overview 与 tutorial 只覆盖 discovery 与 `name` / `description`，没有逐字段表。未证实。
- **Qwen Code 的 `priority`**：只从官方示例看到该顶层字段，取值语义、是否影响排序、未知字段行为均未取得。未证实。
- **Replit、DSH / DeepSeek Harness**：本次没取得字段级文档；DSH 本机安装目录也没能读取（受限 Bash 运行时不可用）。未证实。
- **`builtin.rs` 内置表其余约 60 个工具**（AutoClaw、CodeArts Agent、CodeBuddy、iFlow CLI、Lingma、MiniMax Code、Qoder、Trae、WorkBuddy、ZCode、AdaL、AiderDesk、Antigravity、AstrBot、Augment、Autohand、IBM Bob、Cline、Codemaker、Code Studio、Command Code、Continue、Cortex Code、Crush、Deep Agents、Devin for Terminal、Dexto、Droid、Eve、Firebender、ForgeCode、fx、Grok Build、Hermes、inference.sh、Jazz、Junie、Kilo、Kimchi、Kiro、Kode、Loaf、MCPJam、Mistral Vibe、Moxby、Mux、Neovate、Ona、OpenClaw、OpenHands、Pi、Posit Assistant、Pochi、PromptScript、Reasonix、Roo、Rovo Dev、Sarvam Code、Tabnine、Terramind、Tinycloud、Warp、Windsurf、Zed、Zencoder、Zenflow）：本次只按「有公开 SKILL.md 文档」挑了 12 个常见条目核查，其余未逐一查证。未证实。
- **`metadata` 内层的深度与值类型是否会被某个工具真的拒掉**：规范写「字符串 → 字符串」，但 `skills-ref` 不校验内层、Claude Code 只丢弃「不是 map 的值」、opencode 直接用了布尔值。是否存在某个工具因内层类型报错，未证实。
- **`license` / `compatibility` 的实际消费方**：查到的实现里没有一个拿这两个字段做行为，Claude Code 与 opencode 都明说「接受但不解释」。是否有工具真的读取，未证实。
- **规范正文与 `skills-ref` 代码在 `name` 字符集上的分歧**（正文 `a-z0-9` vs 代码允许 Unicode 字母）哪个是准绳，规范没有裁决说明。未证实。