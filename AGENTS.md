# fuyao-skills-hub 项目规范

skills 管理

**章节归属**（加规则先对号入座）：任务分派与执行 = 队长怎么派活（与走不走流程无关）；开发进度 = 阶段性状态与阶段纪律；开发流程 = 需求入口与流程会话规则（issue、任务清单、票粒度、审查卡点）；通用规范 = 处处成立的规矩（注释、文档语言、git、工具禁令、提交纪律）；Rust、前端（GUI）= 各自技术栈专属；日志规范正文在 `docs/日志规范.md`，本文件只留指针；Agent skills = 只放 `docs/agents/` 指针。

## 任务分派与执行（主代理 + 子代理 / 成员 + 工作树）

主代理是队长，负责任务分派和调度；子代理是队员，负责执行任务。

- **三种执行方式**：**自己做**——任务小、或上下文连贯比拆开更省事时，主代理直接完成，不派单；**串行**——步骤清晰但工作量大时，一次派一个子代理 / 成员做完再派下一个，不建工作树，直接在主仓做；**并行**——任务互相独立时，一个任务一个成员，各在自己的工作树里同时推进，能并行就并行，效率最高。
- **每树自有 target（禁止共享）**：并行任务用 `create_worktree` 建树，成员用 `working_directory` 切进自己的树再动代码。共享 target 时 cargo 的编译单元哈希不区分工作树路径，后编译者会覆盖先编译者的产物，先编译的树再跑 `cargo check` / `cargo clippy` 会被判成"最新"而静默复用别人的结果（实测：改完源码 check 只用 0.17 秒、无 `Checking` 行、产物仍是另一棵树的）。这类假绿在 check 类命令上完全无提示，比 build 出错更危险。

**执行流程**：

1. **确认基线并建树**：先 `git status`，主仓有未提交改动就问用户怎么处理——`create_worktree` 从 HEAD 建树，未提交的改动进不了树。然后 `create_worktree <slug>` 一任务一棵，分支同名，树落在 `.agents/worktrees/<slug>`；建完立刻用 `working_directory` 切回主仓。**建树前 pwd 必须在主仓根**——在树里建树会造出嵌套树，清理极麻烦。
2. **暖编**：按「工作树 target」复制模板；任务要动 `Cargo.toml` / `Cargo.lock` 时模板失效，改拷主仓 `target`（`cp -a target <树>/target`）。每树各自 target。
3. **派任务**：一人一任务，默认并发 3 席；一席完成且验收通过后立即补派下一张票，保持满并发直至票池清空。派任务必须写明第一步：`working_directory` 切进自己的树并 `pwd` 自检——成员默认继承队长当刻的 cwd，不切就会在主仓改代码。
4. **实现**：成员在自己的树里按 `/implement` 做（TDD 驱动；不复杂时走 `/tdd`）。**成员不派子代理**（平台缺陷：二级完成通知唤不醒一级）。审查与提交都等队长复核后再做。
5. **队长调度**：只做派任务、盯进度、答成员问。成员交付后队长派 code-review 双轴审查子代理，报告转成员修硬违规、复跑质量门，复核通过后成员才提交分支。队长不代成员实现或修复。
6. **统一合并**：一批票全部完成后，各成员分支先提交干净，再一条 `git merge <分支1> <分支2> …` 把全部成员分支一次并入队长分支，只产生一个合并提交。
7. **质量门并收尾**：统一合并后跑三项质量门（见「常用命令（Cargo）」的 `cargo fmt` / 带 `--tests` 的 clippy / `cargo test --workspace`），全绿再统一清理本批工作树与分支：`git worktree remove <树>` 删树、`git branch -d <分支>` 删分支，一律不用 rm。

### 工作树 target（并行开发）

每棵工作树建好后先复制一份已编译的 `target` 再开工，不要冷编：

- **模板来源**：任意一棵**源码干净**的工作树跑一次 `cargo check -p fsh-cli --all-targets`，把产出的 `target` 拷到 `.agents/worktrees/.template-target`（工作树池自带 `*` 自忽略规则，不进版本库）。约 50 秒、198 MB；依赖变更（`Cargo.toml` / `Cargo.lock`）后模板失效，重新生成。
- **新树复制**：`cp -a .agents/worktrees/.template-target <新树>/target`，约 3 秒、200 MB；随后 `cargo check` 约 4 秒，只重编本树自己的工作区 crate，依赖全部命中。
- 模板只含 `check` 产物（`.rmeta`），够 `cargo check` / `clippy`；`cargo build` / `test` 需要依赖的 `.rlib`，会重编（实测 25 秒）。不用 sccache 替代：跨工作树 0% 命中，首轮反而更慢（69 秒对 50 秒）。
- 模板必须来自某棵树自己的 `target`，不用共享目录的残留。

## 开发进度

- **阶段开发纪律**：CLI + core 优先，GUI 暂缓开发。技能库这类管理动作由 AI 走 CLI 完成，GUI 只是给人看的视图，所以新能力先落 CLI 与 core、稳定后再考虑 GUI；`src/` 和 `src-tauri/` 冻结，不再改动。
- **GUI 不参与任何工作**：`src/`、`src-tauri/` 与 `fsh-gui` 不进构建图（`Cargo.toml` 的 `members = ["crates/*"]` 已排除）、不编译、不测试、不审查、不列入报告与待办，也不作为缺陷、风险或"遗留"提出——无论那里的代码与 core 有多不一致。只在用户明确要求动 GUI 时才碰；平时连提都不提。
- **文档分工**：`docs/设计文档.md` 只写已交付的功能，未实现的一律写 `docs/未来计划.md`（想法 + 已定决策 + 开工先定的问题）；动工时从 `未来计划.md` 取一条走流程 A，规格建成后把那条整段删除——进了实现的任务不属于未来计划，不留指针。

## 开发流程（需求入口）

新需求一律从下面两条流程之一进入，先按目标清晰度选流程，禁止跳过讨论直接开发：

- **流程 A · 目标明确清晰**（单个特性，一次会话能想清楚）：`/grill-with-docs` 讨论需求（grilling 逐题追问 + domain-modeling 记录术语与决策）直到结论对齐 → **由用户手动**执行 `/to-spec` 生成规格 issue → **由用户手动**执行 `/to-tickets` 拆成带阻塞边的实施 issue → 之后逐票 `/implement`（TDD 驱动、票间清上下文）。两个手动步 agent 只提醒，不代做。
- **流程 B · 目标大而模糊**（绿地项目 / 大特性，一次会话装不下）：`/wayfinder` 建共享地图（`wayfinder:map` issue + 决策子票）逐票推进到迷雾散尽 → 地图清空后同样**由用户手动**执行 `/to-spec`（收拢地图决策为可建计划）→ `/to-tickets` → `/implement`。禁止从地图直接跳 `/implement`（会丢掉决策链细节）；只有工作量确实极小时才允许。

### 流程会话规则

- **规格收口**：规格票下的实施票全部关闭后，agent 在同一次 `deck_issue_patch close` 里带逐项核对评论（对照规格验收项 ✅/❌ + 交付提交号）关闭规格票，不留悬空容器票。
- **票粒度从粗（模块组）**：实施票按模块组切分——一张票把多个相关模块实现到测试全绿才算完成；禁止函数级 / 单文件级细拆；切票前先估票数，能合并成大票就合并，避免流程开销吃掉开发时间。
  - **一张票的下限**：验收面必须包含至少一处**可观察的行为变化**（命令能跑出不同结果、接口多出一种返回、数据形状真的变了）。只是「核对一遍」「再验一次」「把文档抄一遍」「补个测试」的一律不是票——这些是所属实现票或规格收口里的步骤。
  - **同一改动面不跨票**：会改到同一个文件、同一段配置、同一处文档的活，必须放进同一张票。跨票必然带来串行阻塞与合并冲突，代价远大于那张票本身的工作量。
  - **切票前的自检三问**：删掉这张票，功能会不会缺一块？不会就不建票。这张票能否并进相邻那张而不撑破上下文？能就并。这张票是否只是另几张的收尾动作？是就写进那几张的关票评论，不建票。

## 开发规范

### 通用规范

- **注释禁项两条**：
  - 只描述代码本身：不得出现任何"出处/对标"话语——不论动词（参考/对齐/学/借鉴/移植/搬自/对应…，不穷举）其他文件、文档、项目、链接等干扰话语。设计依据要写就重述为代码自身的设计陈述，不带出处。
  - 不写变更历史（辩护词）：只描述当前存在的代码，禁止任何关于"变更本身"的陈述——为什么删除 / 为什么移动 / 已迁至 / 已移出 / 不再属于 / 原先在 等一律禁止。删除即删除、移动即移动，原位置一个字不留；删除/移动的原因只写进提交消息正文。注释的受众是未来读代码的人，不是本次改动的 review 者。改完 .rs 必须自检是否命中这两条禁项。
- **文档语言**：项目一切文档以中文为主——正文中文，仅技术标识符（命令、代码、配置键名、库名、专有名词等）保留原文；英文模板仅作结构骨架参考，使用模版时内容用中文写。
- **零兼容铁律**：兼容一律禁止出现——不写兼容层、兼容读、旧形状或旧字段的兜底、双形状识别、版本探测、迁移代码与迁移工具；规格、票、代码注释、提交消息、文档里也不得出现任何兼容安排或兼容说明。废弃的代码直接删除，不留保留、不留兼容钩子；形状变更一律直切，旧数据由用户自己删掉重建。
- **删除即查残留**：删除公共 API、子命令或错误码时，收口前 `git grep` 全仓残留（含 docs 与 `src-tauri`），连同引用一并清理。
- **临时文件一律写 `.tmp/`**：调试脚本、一次性脚本、临时输出与草稿全部写进项目根的 `.tmp/`（已在 `.gitignore` 中忽略），不存在就建。
- **只读会话禁止发起写操作**：要写就必须问用户切换权限，用户切换后直接执行写操作，禁止通过提权申请写文件。
- **编译类命令遇权限拒绝直接提权**：受限档位（read-only / workspace-write）的 shell 是 Low 完整性，`target` 等产物目录是 Medium，`cargo` / `pnpm` 写产物会报 `拒绝访问 (os error 5)` / `0x800700AA`。这是档位差异，不是文件权限损坏——不跑 ACL 与完整性标签的诊断 / 修复，不改 `target` 标签；直接对那条命令申请一次 `danger-full-access` 执行，执行完回到原档位。此条只管编译、打包与质量门这类要写产物的命令，普通文件写操作仍按上一条。
- **禁用问答弹窗工具**：需要确认或提问时直接在对话里说，禁止调用问答工具。
- **禁用轮询等待**：禁止用 sleep、反复查状态的空转方式等子代理或后台任务，也禁止用阻塞等待工具（`wait_agent` 之类）等子代理——子代理完成任务会直接发消息来，期间继续做不依赖它的工作，禁止空转阻塞会话，最好直接停止回答，等子代理消息或者用户消息。已派给子代理的事不得由主代理代办或抢先做完。
- **推送仅用户主动发起**：`git commit` 可以按流程自行完成；`git push` 一律必须等用户明确主动提出才执行——评审通过、流程走完、工作收尾都不构成推送授权，严禁「顺手推送」；用户授权仅当次有效，不可顺延到后续提交。
- **提交纪律**：默认不提交，改动攒着——要开新主题时，已攒的改动多就先提交再动新主题，新旧改动都很少就合并为一个提交继续攒；同主题小修在所属提交未推时直接 amend 并入；会话中断前可打 WIP 保命提交，事后 `reset --soft` 回收。提交一律 `git add -A`，细微改动（文档措辞、配置、注释）不得排除，只有明显不同主题且用户明确要求拆分时才分开。一个特性的实现、测试与全部伴随文档（设计文档、README、伴生技能、AGENTS.md）攒成**一个**提交，审查与修复完成后一次落下——按文件或按轮次切成多个小提交（「先提交代码，文档回头再补一个提交」「发现措辞要改，再补一个提交」）都不允许。
- 禁用 git stash

### Rust 开发规范

项目采用 **Workspace 依赖统一管理**：所有依赖版本必须在根 `Cargo.toml` 的 `[workspace.dependencies]` 中声明，子 crate 通过 `workspace = true` 引用，禁止在子 crate 中硬编码版本号。开发 Rust 前必须加载 rust-patterns 与 rust-testing skills。

- **加依赖三步**：① `cargo search <包名> --limit 1 --registry crates-io` 查最新（本机 crates-io 被替换为 `rsproxy-sparse` 镜像，不带 `--registry crates-io` 会报 `crates-io is replaced with non-remote-registry source`；该命令需提权档位才能出网）② 在根 `Cargo.toml` 的 `[workspace.dependencies]` 手写依赖及版本号，版本串只写主.次（如 `0.4`、`1`）不写补丁号——查到的完整版本仅用于确认主次仍在最新 ③ 在需要它的子 crate 里 `cargo add <包名> -p <crate名>`：依赖已在 workspace 中会自动写成 `workspace = true`，不在则会硬编码版本号（**禁止**）。

#### 常用命令（Cargo）

- 开发期：`cargo check`（比 build 快，推荐）／ `cargo clippy -- -D warnings`（仅生产代码，快检）／ `cargo fmt`
- 交付前全检：`cargo clippy --workspace --tests -- -D warnings`——`cargo check` 与不带 `--tests` 的 clippy 都不编译 `#[cfg(test)]` 测试代码，测试里的 lint 与漏参 bug 只有加 `--tests` 才暴露，阶段性交付 / 提交前必跑
- 运行测试：`cargo test`；创建 crate：`cargo init --lib crates/fuyao-xxx`

#### 测试规范（Cargo）

- **开发过程避免跑全量测试**：`cargo test --workspace`（尤其含集成测试）耗时长，**除非用户明确要求**，否则开发过程中不要主动运行；开发期验证优先用 `cargo check` 与 `cargo clippy -- -D warnings`；确需跑测试时用 `cargo test -p <crate>` 或 `cargo test -p <crate> --lib` 只跑单元测试。完整测试仅在阶段性交付、提交前、或用户要求时运行。
- **模块形状与测试放置**：一个文件只住一个概念。概念不止一个就用目录模块（`<名>/mod.rs` + 若干子文件），`mod.rs` 只放模块文档、错误类型、共用小件、子模块声明与重导出；单概念模块保持单文件，不造「只有 `mod.rs` + `tests.rs`」的半壳。概念名撞 Rust 关键字时源码文件用下划线避让（`use_.rs` + `mod use_;`）；测试文件名要与被测概念同名而无法加下划线时，用 `#[path = "use.rs"] mod use_flow;` 转一手。
  - 单文件模块：测试内联在该文件末尾的 `#[cfg(test)] mod tests`。
  - 目录模块：测试住同目录的 `tests/` 子目录，按被测概念分文件；共享夹具集中在该子目录的 `mod.rs`，夹具加 `pub(super)`。
  - 测试住在被测代码旁边；任何源码文件不超过约 500 行（测试文件同此标准），超了就按概念再分。
- 单元测试内联 `#[cfg(test)] mod tests { ... }`（目录模块按上一条走 `tests/` 子目录）；集成测试放 `tests/` 目录下，每个文件是独立的测试二进制；异步测试用 `#[tokio::test]`。

### 前端开发规范（GUI）

GUI 前端指 `src/`，技术栈：Tauri 2 + React + TypeScript（strict）+ Vite + Tailwind CSS + shadcn + biome（lint 与格式化）+ pnpm；桌面壳 `src-tauri` 属 Rust 侧，走 Rust 开发规范。

#### 常用命令（GUI）

- 开发调试：`pnpm tauri dev`（前端 + Rust 桥一起跑）
- 编译独立 exe：**由用户手动**执行 `pnpm build:exe`（`tauri build --no-bundle` 后把 `target\release\fuyao-skills-hub.exe` 复制到桌面 `fuyao-skills-hub-v<版本>.exe`）；桌面端编译、打包与启动一律同此——DSH 低权限会改写 `target` 的 ACL，表现为 `LNK1104` 或 `os error 5`，agent 只做代码修改与前端检查（biome / tsc / vitest）
- 生成基础 UI 组件：`pnpm shadcn add <组件>`（落到 `@/components/ui`）；查组件用法：`pnpm shadcn docs <组件>`

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

**写任何日志前先读 `docs/日志规范.md`**——改日志规则只改那一份。

## Agent skills

### Issue tracker

issue 与规格文档都以 Markdown 文件存放在本仓库 `.scratch/` 下，操作用 deck 工具。See `docs/agents/issue-tracker.md`.

### Triage labels

沿用技能默认的五个分诊角色标签字符串。See `docs/agents/triage-labels.md`.

### Domain docs

single-context。See `docs/agents/domain.md`.
