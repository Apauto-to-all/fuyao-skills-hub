---
name: fuyao-integration-test
description: 在 crates/*/tests/ 目录下新建或修改集成测试时使用。规范 fuyao-core 分层架构（L0→L4）的集成测试策略、tests/ 组织约定、mock 决策树（真实集成优先 > 手写 fake > automock）、全局状态规避（set_config 的 OnceLock 单例、环境变量串扰）、风格基线与反例。与管通用测试方法的 rust-testing 职责正交。
---

# Fuyao 集成测试规范

在 fuyao-core 的 `crates/*/tests/` 目录下编写集成测试时强制执行本规范，保证测试组织、mock 策略、
全局状态处理在 11 个 crate 间一致，且重构时能拦截回归。

## 何时使用

* 在 `crates/*/tests/` 下**新建**集成测试文件
* **修改**现有集成测试
* 给某 crate **首次引入**集成测试（建 `tests/` 目录、引入 dev-dependencies）

### 与 rust-testing 的分工

`rust-testing` 管**通用**测试方法（TDD 流程、rstest 语法、mockall 通用用法、proptest、覆盖率工具）。
本 skill 只管 **fuyao 项目专属**约定：分层切入策略、tests/ 组织、mock 决策树、全局状态规避。

写集成测试时两者都要加载——rust-testing 管「怎么写好测试」，本 skill 管「fuyao 的测试该怎么组织」。

## tests/ 组织约定

需要集成测试的 crate 各设 `tests/` 目录，每个 `.rs` 文件是独立测试二进制（Cargo 约定）。
跨测试共享的 fixture（构造函数、tempdir 写入辅助等）放该 crate 的 `tests/common/mod.rs`。

```text
crates/fuyao-xxx/
├── tests/
│   ├── common/
│   │   └── mod.rs        # 共享 fixture（构造函数、文件写入辅助）
│   ├── feature_a_test.rs  # 独立测试二进制
│   └── feature_b_test.rs  # 独立测试二进制
```

**不提前上提为公共 crate**：先在各自的 `tests/common/` 里重复，出现真实跨 crate 复用需求时再考虑提取。
呼应项目「瘦身、避免繁琐」原则——不为「可能复用」提前抽象。

`tests/common/mod.rs` 典型 fixture 模式（字段注入绕开全局状态）：

```rust
// 构造可注入 home 的 AgentPaths，实现 per-test 隔离，零环境变量依赖
pub fn make_agent_paths(home: PathBuf, agent_id: Option<&str>, ws: Option<PathBuf>) -> AgentPaths {
    AgentPaths { agent_id: agent_id.map(str::to_string), workspace: ws, extra_dirs: Vec::new(), fuyao_home: home }
}
```

## 分层测试策略

fuyao-core 是 11 crate 的分层 workspace（L0→L4，依赖严格单向）。集成测试**自下而上**推进，
每层切入点不同：

| 层 | crate | 集成测试切入点 | 主要方式 |
|----|-------|--------------|---------|
| **L0 基础** | fuyao-api | 纯值类型契约（事件协议、配置结构）+ 可注入类型（AgentPaths 路径解析）+ tempdir（配置三层合并加载） | 真实集成，无需 mock |
| **L1 能力** | provider / mcp / skills / hooks | 公开 API 契约；外部不可控依赖（真实 LLM、MCP 子进程）才 mock | 真实集成优先 |
| **L2/L3 内核** | prompt / guard / core / session / tools | 跨模块协作与编排逻辑（ReAct 循环、会话持久化、工具调度、guard 约束） | mock 注入 L1 能力 |
| **L4 装配** | fuyao-app | 四层组装后端到端跑通 | 不可控外部依赖用 mock 替代 |

### 与单元测试的边界

单元测试（源文件内 `#[cfg(test)] mod tests`）管**模块内部逻辑**；集成测试管**跨模块/跨 crate 协作与公共 API 契约**。
两者不重复、不越界——集成测试只触达 crate 的公开 API（根层 `pub use` 导出的符号）。

写不动集成测试 = 公开 API 抽象有问题，这是「最小 pub 接口」在测试侧的反向校验。

## mock 决策树（核心）

集成测试最容易犯的错是「该真实集成的却 mock」和「mock 方式选错」。按以下判别流程决策。

### 第一步：被测能力的依赖是否可控？

| 依赖性质 | 例子 | 决策 |
|---------|------|------|
| 可控（纯值、可注入字段、本地文件系统、内存 SQLite） | 事件类型、AgentPaths、配置加载、SessionStore | **真实集成，不 mock** |
| 不可控（真实外部 API、真实子进程、不可逆副作用） | 真实 LLM API、真实 MCP server、网络请求 | **走 mock** |

能真实组装、真实跑通的**一律走真实路径**。只有外部不可控才 mock。呼应 rust-testing「不要模拟一切」。

### 第二步（需 mock 时）：选对 mock 方式

按被测对象的形态与 trait 复杂度三选一：

**① 手写 fake**（项目默认首选）

适用：trait 返回流（`BoxStream`）、带 `async_trait`、签名复杂、或需要状态化替身。
项目已有先例可复用模式：

```rust
// Provider trait 返回 BoxStream，automock 无法生成——手写 fake 用 stream::iter 构造流
impl Provider for FakeProvider {
    fn stream_chat(&self, _: ChatRequest, _: &str, _: StreamOptions) -> BoxStream<'static, Result<StreamEvent, StreamError>> {
        let events = self.events.clone();
        Box::pin(futures_util::stream::iter(events))
    }
}
```

Plugin trait 同理——手写 fake plugin（记录钩子调用次数、可控返回值）比 automock 直接。

**② automock**（仅 trait 简单时）

适用：trait 无泛型方法、无 `BoxStream`、无 `async_trait`、签名简单。
此时 `mockall::automock` 能正常工作，是最低成本选项。但这是**最后选项，不是默认选项**。

**③ 抽 trait 后 mock**（被测对象是结构体非 trait）

适用：被测对象是结构体（消费方直接持有具体类型，无法替换），如 MCPManager。
此时需先在消费方改用 `Arc<dyn Trait>` 注入点，再 mock。这是**成本最高**的路径，开工前单独评估
「抽 trait」vs「提供 echo/桩服务走端到端」哪个更划算。

### 原则

**优先级：真实集成 > 手写 fake > automock > 抽 trait 重构**。

automock 不是默认选项。强行 automock 一切会引入「假对象与真实行为脱节」的风险，
且 fuyao 多数能力（Provider 的流、Plugin 的 async）automock 本就跑不通。

## 全局状态规避（fuyao 特有）

fuyao 有两个进程级全局状态，集成测试必须规避，否则跨用例串扰、偶发失败。

### set_config 的 OnceLock 单例

`set_config` 基于 `OnceLock`，进程内只能成功一次，重复调用直接 panic。

**规避**：
- 涉及 `set_config` 本身或其 panic 路径的测试，放**独立测试二进制**（独立 `tests/*.rs` 文件）——
  每个文件是独立进程，OnceLock 不跨文件串扰
- 不依赖 set 的测试，走 `get_config` 的「未 set 返回 default」兜底——天然隔离
- **禁止**在同一 `tests/*.rs` 文件里混用「依赖未 set」与「主动 set」的测试

### 环境变量串扰

`load_env` / `get_fuyao_home` 读写 `std::env`，同一测试二进制内多 case 共享进程环境，
并行测试时互相污染。

**规避优先级**：
1. **首选字段注入**：构造被测对象时直接注入路径字段（如 `AgentPaths.fuyao_home`），而非改环境变量。
   fuyao 的路径系统是纯函数，字段可注入即可隔离
2. 不得不改环境变量时：用**进程唯一变量名**（含 `std::process::id()`）+ 测试结束清理

注意：**Rust 2024 edition** 中 `std::env::set_var` / `remove_var` 已标记为 `unsafe`。
调用必须在 `unsafe` 块中，并标注 SAFETY 说明。

```rust
let var_name = format!("FUYAO_TEST_X_{}", std::process::id());
// SAFETY: 单线程测试上下文，var_name 进程唯一，不与其他测试冲突
unsafe { std::env::set_var(&var_name, "value") };
// ... 测试体 ...
// SAFETY: 同上，清理进程唯一变量
unsafe { std::env::remove_var(&var_name) };
```

## 风格基线

与现有单元测试保持一致（集成测试与单元测试是同一套代码风格的两种组织形式）：

* **命名**：`snake_case`，描述性，**不带 `test_` 前缀**（如 `three_layer_precedence_workspace_overrides_global`）
* **断言**：`assert_eq!` / `assert!`，panicking 风格
* **错误路径**：优先 `Result::is_err()` 与错误变体匹配（`matches!(err, ConfigError::TomlError(_))`），
  不用 `#[should_panic]`——除非被测 API 本身以 panic 表达契约（如 `set_config` 的重复调用）
* **异步**：`#[tokio::test] async fn`
* **参数化**：rstest 的 `#[rstest]` + `#[values(...)]`，表驱动场景（枚举变体、配置组合、路径形式）
* **fixture**：tempfile 的 `tempdir()` 做文件系统隔离，自动清理

**覆盖率是参考线，非唯一 KPI**：关键业务逻辑与公共 API 覆盖率优先，通用代码达标即可。
不为凑覆盖率写无意义测试。

## 反例（不应出现）

* 在 async 上下文做阻塞调用（`std::thread::sleep`、阻塞 IO）——用 `tokio::time::sleep` / 异步 API
* 为「可能复用」提前把 `tests/common/` 上提为公共测试 crate
* 默认 automock 一切——能真实集成的（纯值类型、可注入字段、本地文件系统）不要 mock
* 在同一 `tests/*.rs` 文件里混用依赖不同 OnceLock 状态的测试（一个 set、一个依赖未 set）
* 改环境变量不清理、不用进程唯一变量名——会导致偶发并行测试失败
* 集成测试与单元测试重复覆盖同一模块内部逻辑——集成测试应聚焦跨模块协作与公共 API 契约
* 用 `#[should_panic]` 测本可用 `Result::is_err()` 表达的错误路径
* 在 Rust 2024 下用 `std::env::set_var` 不加 `unsafe` 块与 SAFETY 注释
* 把集成测试写成只触达内部模块（`mod` 私有项）——集成测试只能用公开 API（根层 `pub use`）
