# fuyao-skills-hub 项目规范

skills 管理

## 章节定义（往哪一节加规则，先对号入座）

- **开发进度**：阶段性状态与阶段纪律（当前只做什么、何时解锁什么）
- **流程会话规则**：只在走 matt 流程时适用的行为规则
- **通用规范**：与流程无关、任何会话都成立的规矩（注释、文档语言、git 行为、工具禁令、提交纪律等）
- **Rust 开发规范 / 测试规范**：仅 Rust 与 cargo 相关的规矩
- **前端开发规范（GUI）**：仅 GUI 前端（`src/`）相关的规矩
- **日志规范**：日志专用
- **Agent skills**：技能配置指针，只指向 `docs/agents/`，不放规则正文

判据：流程专属 → 会话规则；处处适用 → 通用规范；技术栈专属 → 对应技术规范节。

## 开发进度

- **阶段开发纪律**：CLI + core 优先，GUI 暂缓开发。技能库这类管理动作由 AI 走 CLI 完成，GUI 只是给人看的视图，所以新能力先落 CLI 与 core、稳定后再考虑 GUI；src/ 和 src-tauri/ 冻结，不再改动
- **文档分工**：`docs/设计文档.md` 只写已交付的功能，未实现的一律写 `docs/未来计划.md`（想法 + 已定决策 + 开工先定的问题）；动工时从 `未来计划.md` 取一条走流程 A，规格建成后把那条整段删除——进了实现的任务不属于未来计划，不留指针

## 开发流程（需求入口）

新需求一律从下面两条流程之一进入，先按目标清晰度选流程，禁止跳过讨论直接开发：

### 流程 A：目标明确清晰（单个特性，一次会话能想清楚）

1. `/grill-with-docs` 讨论需求（grilling 逐题追问 + domain-modeling 记录术语与决策），直到结论对齐
2. 对齐后**由用户手动**执行 `/to-spec`，把结论生成规格 issue（agent 只提醒，不代做）
3. 规格 issue 生成后**由用户手动**执行 `/to-tickets`，拆成带阻塞边的实施 issue（agent 只提醒，不代做）
4. 之后逐票 `/implement`（TDD 驱动、票间清上下文）

### 流程 B：目标大而模糊（绿地项目 / 大特性，一次会话装不下）

1. `/wayfinder` 建共享地图（`wayfinder:map` issue + 决策子票），逐票推进直到迷雾散尽
2. 地图清空后同样**由用户手动**执行 `/to-spec`（收拢地图决策为可建计划）→ `/to-tickets` → `/implement`
3. 禁止从地图直接跳 `/implement`（会丢掉决策链细节）；只有工作量确实极小时才允许

### 流程会话规则

- **先查后问**：提问前先阅读仓内相关内容（文档、代码、相关 issue），并用一句话说明已查阅范围
- **复用既有笔记**：优先复用仓内已有研究笔记，复用时只记录路径与一句话结论，不粘贴全文；既有笔记覆盖不了时按 `/research` 规则新查一次，说明不适用原因，新笔记路径回填正文
- **issue 工具**：issue 相关操作优先使用 `deck_` 开头工具；deck 工具异常时按当前工作区选择合适方式兜底
- **及时上报**：处理过、处理中的 issue 用 `deck_issue_report` 上报编号
- **实施票先认领，认领落盘才动代码**：拿到一张实施票，第一步永远是认领——`deck_issue_patch` 设 `Assignees`，并把正文的 `**Status:**` 从 `ready-for-agent` 改成 `claimed`。认领没写进票文件之前，禁止修改任何代码。跳过认领直接实现，本次实施按流程未完成处理
- **长任务必须用任务清单**：一张票几乎总是多步（读全 → 写测试 → 实现 → 质量门 → 审查 → 提交 → 关票）。开工前先建任务清单，做到哪一步就更新哪一步，不许只在脑内推进，也不许等全部做完才补记。用户看不到过程就等于没做
- **规格收口**：规格票下的实施票全部关闭后，agent 在同一次 `deck_issue_patch close` 里带逐项核对评论（对照规格验收项 ✅/❌ + 交付提交号）关闭规格票，不留悬空容器票
- **票粒度从粗（模块组）**：实施票按模块组切分——一张票把多个相关模块实现到测试全绿才算完成；禁止函数级 / 单文件级细拆。切票前先估票数，能合并成大票就合并，避免流程开销吃掉开发时间
- **实现阶段用团队**：切票后多张实施票互相独立时，可用宿主当时提供的多智能体团队能力并行推进，在切票阶段要尽可能让其可并行并且互不干扰，避免文件范围重叠导致的冲突。只有**文件范围互不重叠**的票才并行，重叠的串行实现即可；团队成员同受本文件全部规范约束（零兼容、注释、测试、提交纪律）
- **收尾推荐**：回复前最后一步按 /ask-matt 规则评估当前最合适的下一步技能，调用 `suggest_next_steps` 推荐
- **上下文卫生**：grill → spec → tickets 尽量在同一未断上下文窗口内完成；每张实施票独立新开上下文
- **实施收尾必须子代理审查（卡点）**：每张实施票在**提交与关票之前**，必须按 /code-review 规则并行起两个子代理（Standards 轴对照本文件规范 + Fowler 基线、Spec 轴对照所属规格票）审查该票完整 diff；报告里判为硬违规的先修复并复跑质量门，再提交、关票。跳过此步直接提交关票视为流程未完成

## 开发规范

### 通用规范

- 注释必须保留或更完善：禁止删除原有注释，新代码必须添加完整注释
- 注释只描述代码本身：禁止在代码注释里出现任何"出处/对标"话语——不论动词（参考/对齐/学/借鉴/移植/搬自/对应…，不穷举）其他文件、文档、项目、链接等干扰话语。设计依据要写就重述为代码自身的设计陈述，不带出处。
- 禁止变更历史注释（辩护词）：注释只描述当前存在的代码，禁止出现任何关于"变更本身"的陈述——为什么删除 / 为什么移动 / 已迁至 / 已移出 / 不再属于 / 原先在 等一律禁止。删除即删除、移动即移动，原位置一个字不留；删除/移动的原因只写进提交消息正文。注释的受众是未来读代码的人，不是本次改动的 review 者。改完 .rs 必须自检注释是否命中上述两条禁项。
- 文档语言：项目一切文档以中文为主——正文中文，仅技术标识符（命令、代码、配置键名、库名、专有名词等）保留原文；英文模板仅作结构骨架参考，使用模版时内容用中文写
- **零兼容铁律**：兼容一律禁止出现——不写兼容层、兼容读、旧形状或旧字段的兜底、双形状识别、版本探测、迁移代码与迁移工具；规格、票、代码注释、提交消息、文档里也不得出现任何兼容安排或兼容说明。废弃的代码直接删除，不留保留、不留兼容钩子；形状变更一律直切，旧数据由用户自己删掉重建。
- 项目干净：不被使用的函数、多余的导出，**先判断**是否有价值，如果有就保留，并且用 `// TODO:` 标记并说明用途，如果该代码目前不使用，可以使用 `//` 把代码完整注释掉，便于后续直接启用。对于废弃代码，无用代码，死代码，直接删除
- 只读会话禁止发起写操作，要写就必须问用户切换权限，用户切换后直接执行写操作，禁止通过提权申请写文件
- **编译类命令遇权限拒绝直接提权**：受限档位（read-only / workspace-write）的 shell 是 Low 完整性，`target` 等产物目录是 Medium，`cargo` / `pnpm` 写产物会报 `拒绝访问 (os error 5)` / `0x800700AA`。这是档位差异，不是文件权限损坏——不跑 ACL 与完整性标签的诊断 / 修复，不改 `target` 标签；直接对那条命令申请一次 `danger-full-access` 执行，执行完回到原档位。此条只管编译、打包与质量门这类要写产物的命令，普通文件写操作仍按上一条走
- 禁用问答弹窗工具：需要确认或提问时直接在对话里说，禁止调用问答工具
- 禁用轮询等待：禁止用 sleep、反复查状态的空转方式等子代理或后台任务。要结果就阻塞式读取（子代理完成会自行回报）；不要结果就继续做其它不依赖它的工作，禁止空转阻塞会话。已派给子代理的事不得由主代理代办或抢先做完
- **推送仅用户主动发起（铁律）**：`git commit` 可以按流程自行完成；`git push` 一律必须等用户明确主动提出才执行——评审通过、流程走完、工作收尾都不构成推送授权，严禁「顺手推送」；用户授权仅当次有效，不可顺延到后续提交
- **提交纪律**：修改后默认不提交，改动攒着。要开新主题时：已攒的改动多，先提交再动新主题；新旧改动都很少，合并为一个提交继续攒。同主题小修在所属提交未推时直接 amend 并入；会话中断前可打 WIP 保命提交，事后 `reset --soft` 回收；推送始终须用户发起
- **提交默认打包全部改动**：提交一律 `git add -A`；细微改动（文档措辞、配置、注释）不得排除，跟随所属改动一起提交；只有明显不同主题且用户明确要求拆分时才分开提交
- **禁用 git stash（铁律）**：禁止用 `git stash` 暂存、搬运或收起改动——改动只有两条出路：按提交纪律提交，或经用户明确同意后丢弃。还原未提交的改动用逐处反改（edit 回退到原文），或先问用户；改动绝不藏进 stash

### Rust 开发规范

项目采用 **Workspace 依赖统一管理**，所有依赖版本必须在根 `Cargo.toml` 的 `[workspace.dependencies]` 中声明，子 crate 通过 `workspace = true` 引用，禁止在子 crate 中硬编码版本号。

开发 Rust 前，必须加载：rust-patterns 和 rust-testing skills；开发修改rust文件后，需要使用 `cargo fmt` 格式化代码

- 搜索最新版本：`cargo search <包名> --limit 1 --registry crates-io`（本机 crates-io 被替换为 `rsproxy-sparse` 镜像，不带 `--registry crates-io` 会报 `crates-io is replaced with non-remote-registry source`；该命令需提权档位才能出网）
- 手动编辑根 `Cargo.toml`，在 `[workspace.dependencies]` 中添加依赖及版本号；版本串只写主.次（如 `0.4`、`1`），不写补丁号——`cargo search` 查到的完整版本仅用于确认主次仍在最新
- 在需要该依赖的子 crate 中引用：`cargo add <包名> -p <crate名>`
  - 如果该依赖已在 `[workspace.dependencies]` 中，此命令会自动使用 `workspace = true` 形式
  - 如果该依赖不在 workspace 中，此命令会直接在子 crate 中硬编码版本号（**禁止**）

#### 常用命令（Cargo）

- 运行检查：`cargo check`（比 build 快，推荐开发时使用）
- 运行测试：`cargo test`
- 格式化代码：`cargo fmt`
- 静态检查：`cargo clippy -- -D warnings`（仅生产代码，开发期快检）
- 交付前全检：`cargo clippy --workspace --tests -- -D warnings`（含 `#[cfg(test)]` 测试代码，阶段性交付 / 提交前必跑）
- 覆盖率：`cargo llvm-cov --fail-under-lines 80`
- 创建各个 crate： `cargo init --lib crates/fuyao-xxx`       # lib crate

#### 测试规范（Cargo）

- **开发过程避免跑全量测试**：`cargo test --workspace`（尤其含集成测试）耗时长，**除非用户明确要求**，否则开发过程中不要主动运行。开发期验证优先用 `cargo check`（快）和 `cargo clippy -- -D warnings`（静态检查）；确需跑测试时，用 `cargo test -p <crate>` 限定单个 crate 或 `cargo test -p <crate> --lib` 只跑单元测试。完整测试仅在阶段性交付、提交前、或用户要求时运行。
- **交付前 clippy 必含 `--tests`**：`cargo check` 和不带 `--tests` 的 `cargo clippy` 都**不编译 `#[cfg(test)]` 测试代码**，测试里的 clippy lint 与漏参 bug 只有加 `--tests` 才会暴露。阶段性交付 / 提交前，务必跑 `cargo clippy --workspace --tests -- -D warnings`。
- 运行测试：`cargo test --workspace`（包含所有 crate 的测试）
- **模块形状与测试放置**：一个文件只住一个概念。概念不止一个就用目录模块（`<名>/mod.rs` + 若干子文件），`mod.rs` 只放模块文档、错误类型、共用小件、子模块声明与重导出；单概念模块保持单文件，不造「只有 `mod.rs` + `tests.rs`」的半壳。概念名撞 Rust 关键字时，源码文件用下划线避让（`use_.rs` + `mod use_;`）；测试文件名要与被测概念同名而无法加下划线时，用 `#[path = "use.rs"] mod use_flow;` 转一手。
  - 单文件模块：测试内联在该文件末尾的 `#[cfg(test)] mod tests`。
  - 目录模块：测试住同目录的 `tests/` 子目录，按被测概念分文件；共享夹具集中在该子目录的 `mod.rs`，夹具加 `pub(super)`。
  - 测试住在被测代码旁边；任何源码文件不超过约 500 行（测试文件同此标准），超了就按概念再分。
- 单元测试：在源文件内使用 `#[cfg(test)] mod tests { ... }`（目录模块按上一条走 `tests/` 子目录）
- 集成测试：放在 `tests/` 目录下，每个文件是独立的测试二进制
- 异步测试：使用 `#[tokio::test]`
- 覆盖率目标：关键业务逻辑 100%，公共 API 90%+，通用代码 80%+
- rust 项目需要在源码实现单元测试
- **测试临时目录一律 `tempfile::tempdir()`**：夹具不得手工在仓库或固定路径建测试目录；`TempDir` 守卫必须被持有到测试结束（不得只拿路径丢守卫），离开作用域自动整树清理。已知残留源：低权限沙箱里 Drop 删除失败且静默忽略，垃圾会落在 `.tmp\tmp\`——沙箱里跑完测试要顺手检查并清掉该目录

### 前端开发规范（GUI）

GUI 前端指 `src/`，技术栈：Tauri 2 + React + TypeScript（strict）+ Vite + Tailwind CSS + shadcn + biome（lint 与格式化）+ pnpm；桌面壳 `src-tauri` 属 Rust 侧，走 Rust 开发规范。

#### 常用命令（GUI）

- 开发调试：`pnpm tauri dev`（前端 + Rust 桥一起跑）
- 编译独立 exe：**由用户手动**执行 `pnpm build:exe`（`tauri build --no-bundle` 后把 `target\release\fuyao-skills-hub.exe` 复制到桌面 `fuyao-skills-hub-v<版本>.exe`）
- **编译、打包与启动桌面端一律由用户手动操作，agent 不代跑**：DSH 低权限会改写 `target` 的 ACL，表现为 `LNK1104` 或 `os error 5`；agent 只做代码修改与前端检查（biome / tsc / vitest）
- 生成基础 UI 组件：`pnpm shadcn add <组件>`（落到 `@/components/ui`）
- 查组件用法：`pnpm shadcn docs <组件>`

#### 格式化与检查（GUI）

- 改完 `src/` 或前端配置文件后，必须跑 `pnpm biome check --write`（lint + 格式化 + import 排序），提交前确认无报错
- 类型检查用 `pnpm exec tsc`（`tsconfig.json` 已设 `noEmit`，只查 `src/`；`vite.config.ts` 不在其内）
- biome 的检查范围写在 `biome.json` 的 `files.includes`，新增顶层配置文件时同步加进去

#### 依赖与目录（GUI）

- 依赖一律用 `pnpm add` / `pnpm add -D` 添加，不手改 `package.json`；`pnpm-lock.yaml` 随提交一起走
- 构建期工具（biome、shadcn CLI、tauri CLI、tailwind）进 devDependencies
- 目录归属：`src/components/ui/` 只放 `pnpm shadcn add` 生成的组件，自研组件放 `src/components/`，工具函数放 `src/lib/`；`@/` 指向 `src/`，别名以 `components.json` 为准
- `cn` 只有一个来源（`cn` 包）：自研组件经 `@/lib/utils` 取用，`pnpm shadcn add` 生成的组件保持生成时的写法（`import { cn } from "cn"`），不手改

### 日志规范

日志地基已落地（`crates/fsh-core/src/logging.rs`）：文件层写 hub 根下 `logs\`（按天滚动，只保留 30 天，`.gitignore` 已排除）+ stderr 层；非阻塞写入，`init_logging` 返回的 `WorkerGuard` 由宿主持到进程结束（CLI 在 `process::exit` 前手动 drop）。级别固定 `info`，`RUST_LOG` 可覆盖；CLI 与桌面端各调用一次。所有 crate 写日志须遵循以下规范。

**三条铁律**：① 事件而非流（一条日志 = 一个有意义事件，不是每行都记）② 结构化（字段 `key=value`，禁止拼进消息）③ 5W 齐全（时间戳 / target / 消息 / 字段 / 结果）。

**级别语义**：

| 级别 | 用途 | 本项目场景 |
| ------ | ------ | --------- |
| ERROR | 需人介入，功能受损 | Fuyao Skills Hub 初始化失败、git 仓库损坏、配置无法读写 |
| WARN | 可疑 / 可恢复 | 单条 skill 解析失败被跳过、回收删不掉文件、锁被占用、日志层降级为 stderr |
| INFO | 业务里程碑 / 外部交互 / 决策 | 入库、发版、归档、移除、恢复、项目使用 / 解除使用、回收、Fuyao Skills Hub 初始化、CLI 命令完成 |
| DEBUG | 诊断细节 | git 命令与耗时、目标路径计算、链接方式选择、查询命中数 |
| TRACE | 极细粒度 | 一般不用 |

**两条黄金法则**：① 半夜叫醒测试——只有 ERROR 触发报警，可恢复失败一律 WARN；② INFO 重建故事——读 INFO 能重建系统做了什么，翻屏说明把 DEBUG 误当 INFO。

**记录点位**（只在这五类点记）：

1. 外部交互：git 命令、批量文件系统操作
2. 状态迁移：入库 / 发版 / 归档 / 移除 / 恢复 / 使用 / 解除使用 / 回收
3. 决策：幂等命中、跳过、降级、链接方式选择
4. 失败与重试：错误只在归属它的边界记一次，上层不重复记
5. 耗时：跨多次外部调用的操作带 `elapsed_ms`

**消息与字段**：

- 消息是中文陈述句、动词收尾、不加标点：`"skill 入库完成"`、`"版本发布完成"`、`"回收删除文件"`。
- 消息不重复字段内容：名词进字段，动词进消息。
- 指代本项目时写阅读写法 `Fuyao Skills Hub`；`fuyao-skills-hub` 是仓库与包名写法，只出现在路径、包名、命令里，不进句子。
- 固定字段：`key`（skill 键）、`version`、`project`、`path`、`count`、`elapsed_ms`、`cause`（错误原因）、`ok`（布尔）。
- 值用 `%`（Display）或裸值；路径写 `path = %p.display()`，错误写 `cause = %e`。
- 不用 `#[instrument]`：它记录全部参数，易泄露内容且刷屏。

```rust
tracing::info!(key = %key, version = %v, elapsed_ms = elapsed, "skill 入库完成");
tracing::warn!(path = %p.display(), cause = %e, "跳过无法解析的 skill");
```

**可查询性**（格式为查询而设计，改日志时先对这几条）：

- 单行形态：`2026-11-05 04:33:31.123  INFO fsh_core::skill::add: skill 入库完成 key=mattpocock/skills/tdd version=1.2.0 elapsed_ms=12`。按 `target` 前缀筛模块（`grep 'fsh_core::skill'`），按消息后缀筛结果。
- 消息后缀只用五个词，且与级别绑定：`完成`→INFO，`失败`→WARN 或 ERROR，`跳过`→WARN，`降级`→WARN，`删除`→INFO。于是 `grep 失败` 一次捞出全部异常，`grep 完成` 得到完整成功故事。约束只针对 INFO / WARN / ERROR；DEBUG 是诊断细节，后缀不受限。
- 字段顺序固定：标识（`key` / `project` / `path`）→ 结果（`count` / `ok`）→ 耗时（`elapsed_ms`）→ 原因（`cause` 永远最后）。
- 路径一律记绝对路径（`%p.display()`），不记相对路径。
- 同一事件只记一条，禁止在多层重复记：重复行会让 grep 结果失真。
- 时间戳是本地时间毫秒精度，字典序即时间序，可直接 `sort` / `tail`。

**敏感信息红线**：禁止记录 API Key / Token / 用户输入全文 / 文件内容 / 含密钥命令。错误信息含密钥需脱敏。

**高频热路径不记 INFO**：流式 chunk / 循环每次迭代用 DEBUG 或不记。

## Agent skills

### Issue tracker

issue 与规格文档都以 Markdown 文件存放在本仓库 `.scratch/` 下，操作用 deck 工具。See `docs/agents/issue-tracker.md`.

### Triage labels

沿用技能默认的五个分诊角色标签字符串。See `docs/agents/triage-labels.md`.

### Domain docs

single-context。See `docs/agents/domain.md`.
