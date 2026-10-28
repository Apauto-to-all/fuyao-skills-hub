# fuyao-skills-hub 项目规范

skills 管理

## 开发进度

- **阶段开发纪律**：当前只开发 `fsh-core` 模块；`fsh-core` 达到 MVP 基础可用后，再 CLI（`fsh-cli`）+ GUI（`src-tauri` / `src`）+ core 三线同步开发。core 未达标前，不动 CLI 与 GUI 的业务代码

## 开发规范

### 通用规范

- 注释必须保留或更完善：禁止删除原有注释，新代码必须添加完整注释
- 注释只描述代码本身：禁止在代码注释里出现任何"出处/对标"话语——不论动词（参考/对齐/学/借鉴/移植/搬自/对应…，不穷举）其他文件、文档、项目、链接等干扰话语。设计依据要写就重述为代码自身的设计陈述，不带出处。
- 禁止变更历史注释（辩护词）：注释只描述当前存在的代码，禁止出现任何关于"变更本身"的陈述——为什么删除 / 为什么移动 / 已迁至 / 已移出 / 不再属于 / 原先在 等一律禁止。删除即删除、移动即移动，原位置一个字不留；删除/移动的原因只写进提交消息正文。注释的受众是未来读代码的人，不是本次改动的 review 者。改完 .rs 必须自检注释是否命中上述两条禁项。
- 文档语言：项目一切文档以中文为主——正文中文，仅技术标识符（命令、代码、配置键名、库名、专有名词等）保留原文。第三方 skill 自带的英文 seed 模板仅作结构骨架参考，内容必须用中文重写，禁止原样复制英文产物
- 开发阶段，禁止做任何兼容处理，对于废弃的代码，直接删除，禁止进行兼容保留
- 项目干净：不被使用的函数、多余的导出，**先判断**是否有价值，如果有就保留，并且用 `// TODO:` 标记并说明用途，如果该代码目前不使用，可以使用 `//` 把代码完整注释掉，便于后续直接启用。对于废弃代码，无用代码，死代码，直接删除
- 只读会话禁止发起写操作，要写就必须问用户切换权限，用户切换后直接执行写操作，禁止通过提权申请写文件

### Rust 开发规范

项目采用 **Workspace 依赖统一管理**，所有依赖版本必须在根 `Cargo.toml` 的 `[workspace.dependencies]` 中声明，子 crate 通过 `workspace = true` 引用，禁止在子 crate 中硬编码版本号。

开发 Rust 前，必须加载：rust-patterns 和 rust-testing skills；开发修改rust文件后，需要使用 `cargo fmt` 格式化代码

- 搜索最新版本：`cargo search <包名> --limit 1`，获取最新版本号
- 手动编辑根 `Cargo.toml`，在 `[workspace.dependencies]` 中添加依赖及版本号（使用搜索得到的最新版本）
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
- 单元测试：在源文件内使用 `#[cfg(test)] mod tests { ... }`
- 集成测试：放在 `tests/` 目录下，每个文件是独立的测试二进制
- 编写或修改集成测试（`tests/` 目录）前，必须加载 `fuyao-integration-test` skill
- 异步测试：使用 `#[tokio::test]`
- 覆盖率目标：关键业务逻辑 100%，公共 API 90%+，通用代码 80%+
- rust 项目需要在源码实现单元测试

### 日志规范 (功能待实现)

日志地基（tracing subscriber + `[logging]` 配置 + per-agent 文件 + guard）已落地，所有 crate 写日志须遵循以下规范。

**三条铁律**：① 事件而非流（一条日志 = 一个有意义事件，不是每行都记）② 结构化（字段 `key=value`，禁止拼进消息）③ 5W 齐全（时间戳 / target / 消息 / 字段 / 结果）。

**级别语义**：

| 级别 | 用途 | 典型场景 |
| ------ | ------ | --------- |
| ERROR | 需人介入，功能受损 | Provider 创建失败、MCP 启动失败 |
| WARN | 可疑 / 可恢复 | LLM 重试、MCP 熔断、压缩触发、插件 panic 恢复 |
| INFO | 业务里程碑 / 外部交互 / 决策 | 引擎初始化、LLM 耗时、工具调用、会话创建 |
| DEBUG | 诊断细节 | 流式 chunk、中间状态 |
| TRACE | 极细粒度 | 一般不用 |

**两条黄金法则**：① 半夜叫醒测试——只有 ERROR 触发报警，可恢复失败一律 WARN；② INFO 重建故事——读 INFO 能重建系统做了什么，翻屏说明把 DEBUG 误当 INFO。

**字段命名**：耗时 `elapsed_ms`、计数明确名词（`attempt`/`turn`）、布尔形容词（`ok`/`recovered`）、错误原因用 `cause`（非 `error`），统一 `snake_case`。消息用中文陈述句。

**敏感信息红线**：禁止记录 API Key / Token / 用户输入全文 / 文件内容 / 含密钥命令。错误信息含密钥需脱敏。

**高频热路径不记 INFO**：流式 chunk / 循环每次迭代用 DEBUG 或不记。

## Agent skills

### Issue tracker

issue 与规格文档都在 GitHub Issues（Apauto-to-all/fuyao-skills-hub），操作用 `gh`。See `docs/agents/issue-tracker.md`.

### Triage labels

沿用技能默认的五个分诊角色标签字符串。See `docs/agents/triage-labels.md`.

### Domain docs

single-context。See `docs/agents/domain.md`.
