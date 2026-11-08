# M3 Git 远端同步与 M4 agent 内置表：一手事实

- 调研日期：2026-11-03
- 对应设计：M3（远端同步，未实现，见 `docs/未来计划.md` P11）；M4（agent 配置，已交付，见 `docs/设计文档.md` §2.6）
- 范围：`git2` 0.21.0 的网络、认证、push、fetch 与合并能力；`vercel-labs/skills` 当前 `src/agents.ts` 的注册表字段和维护方式。
- 来源仅为官方 API 文档、官方仓库源码与提交记录。下文“事实”与“建议”分开，不展开完整实现方案。

## 一、M3：`git2` 0.21.0

### 1. 网络传输 features 与构建

**事实**

- 0.21.0 的 `default` feature 不启用任何其他 feature；`https`、`ssh` 必须显式打开。`https` 连带启用 `cred`、`libgit2-sys/https`、`openssl-probe`、`openssl-sys`；`ssh` 连带启用 `cred`、`libgit2-sys/ssh`。因此默认构建不能据此宣称支持 HTTPS/SSH 远端。  
  原始来源：https://docs.rs/crate/git2/0.21.0/features
- `vendored-libgit2` 和 `vendored-openssl` 分别把构建转向 vendored libgit2 / OpenSSL；`https` 本身并不自动等价于 `vendored-openssl`。若发布环境要求静态携带这些原生组件，需要分别核对 feature，而不能只看 Rust API 是否存在。  
  原始来源：https://docs.rs/crate/git2/0.21.0/features
- `FetchOptions` 可配置认证回调、代理、prune、FETCH_HEAD、depth、tag 下载策略、重定向和自定义 header；`PushOptions` 可配置认证回调、代理、pack 并行度、重定向、自定义 header 和远端 push options。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.FetchOptions.html  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.PushOptions.html

### 2. 认证

**事实**

- 远端操作通过 `RemoteCallbacks::credentials` 按需返回 `Cred`；回调能收到 URL、URL 中的用户名，以及允许的 `CredentialType`，不是只在进程启动时读取一次凭据。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.RemoteCallbacks.html#method.credentials
- `Cred` 提供用户名/明文密码、SSH agent、磁盘 SSH key、内存 SSH key、仅用户名，以及 Git credential helper。HTTPS 私有库常用的 token 在 API 层走 `userpass_plaintext(username, password)`；git2 没有名为“GitHub OAuth/device flow”的高级登录 API。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Cred.html
- `Cred::credential_helper` 会按 `credential.helper` 配置调用必要进程并读取用户名/密码；这不是把 Git Credential Manager 静默内嵌进 Rust 库。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Cred.html#method.credential_helper
- TLS 证书校验失败会进入 `certificate_check`，由调用方决定是否继续；是否放宽校验属于调用方选择，不是必须关闭的安全步骤。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.RemoteCallbacks.html#method.certificate_check

### 3. push：能做什么、要额外检查什么

**事实**

- `Remote::push(refspecs, opts)` 执行 push；refspecs 为空时使用配置中的 refspecs。推送分支与推送 tag 都由调用方给出相应 refspec，不存在“push 当前分支就必然同步全部本地 tag”的 API 保证。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Remote.html#method.push
- 官方文档特别提示应设置 `push_update_reference` 检查每个引用是否真正成功：回调第二参数为 `Some(status)` 表示该引用被远端拒绝。因此不能把 `Remote::push` 返回 `Ok(())` 单独当成所有引用均已更新。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Remote.html#method.push  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.RemoteCallbacks.html#method.push_update_reference
- `push_negotiation` 可查看待发送更新并取消；也有 push 传输进度回调。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.RemoteCallbacks.html

### 4. pull：fetch 与整合是两个阶段

**事实**

- `Remote::fetch` 的便利函数会连接远端、下载对象、断开并更新 remote-tracking branches；它不会替代调用方完成当前分支整合。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Remote.html#method.fetch
- fetch 默认自动跟随指向已下载对象的 tag；`AutotagOption::All` 才表示请求全部 tag。对以 git tag 为版本真身的 hub，默认跟随策略不能当作“完整同步所有版本引用”的充分条件。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.FetchOptions.html#method.download_tags  
  原始来源：https://docs.rs/git2/0.21.0/git2/enum.AutotagOption.html
- `Repository::annotated_commit_from_fetchhead`、`merge_analysis`、`merge`、`checkout_head`、`Reference::set_target` 等底层能力均存在；`MergeAnalysis` 区分 `UP_TO_DATE`、`FASTFORWARD`、`NORMAL`（双方分叉）、`UNBORN`、`NONE`。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Repository.html  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.MergeAnalysis.html
- fast-forward 的分析结果只说明无需 merge commit、客户端可检出目标；更新本地引用及工作树仍是调用方工作。`Repository::merge` 只把合并结果放进 index，不创建 merge commit，也不保证工作树与冲突处理已结束。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.MergeAnalysis.html#associatedconstant.ANALYSIS_FASTFORWARD  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Repository.html#method.merge
- `Remote`、`RemoteCallbacks`、`Cred` 均为 `!Send + !Sync`；这与能否执行网络操作无关，但会约束 Rust/GUI 的线程封装。  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Remote.html  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.RemoteCallbacks.html  
  原始来源：https://docs.rs/git2/0.21.0/git2/struct.Cred.html

### 5. 面向 M3 的建议（非完整方案）

- 先明确验收中的“版本一致”包含当前分支、所有版本 tag，还是二者；在 refspec 与逐引用结果检查中体现，不能只同步分支。
- pull 在 dirty worktree、up-to-date、fast-forward、双方分叉、冲突下分别定义行为；不要把这些情况压成一次 `fetch` 的返回值。
- 认证策略应结合 portable 与“不写家目录”立场单独拍板；尤其 credential helper、SSH agent 和 key 路径带来的外部依赖不应被描述成零运行时依赖。
- Windows 发布构建需实际验证 HTTPS/SSH 和静态分发，不能仅依据 feature 名称推断产物依赖。

## 二、M4：上游 `src/agents.ts`

### 1. 注册表的真实形状

**事实**

- 当前文件维护 `export const agents: Record<AgentType, AgentConfig>` 静态对象，不是运行时下载的 JSON，也没有自动同步机制。每项手写配置，通常还带一个异步 `detectInstalled` 函数。  
  原始源码：https://github.com/vercel-labs/skills/blob/main/src/agents.ts
- `AgentConfig` 的完整字段是：`name`、`displayName`、`skillsDir`、`globalSkillsDir`、`detectInstalled`，以及可选的 `showInUniversalList`、`showInUniversalPrompt`、`createProjectSkillsDirByDefault`。`globalSkillsDir` 可为 `undefined`。  
  原始类型：https://github.com/vercel-labs/skills/blob/main/src/types.ts
- 对本项目 M4 已有数据形状，直接相关的是对象键 / `name`、`displayName` 和项目相对目录 `skillsDir`。其余字段表达全局安装、安装检测、上游交互展示或自动建目录行为，不属于本项目已定的内置 TOML 两字段。  
  字段事实来源：https://github.com/vercel-labs/skills/blob/main/src/types.ts
- 上游把 `.agents/skills` 当通用目录：`getUniversalAgents` 按 `skillsDir === '.agents/skills'` 分类，非通用目录走 agent 专属位置。因此“通用”在这里是按值归类的判断，而非另一条特殊 agent 配置规则。  
  原始源码：https://github.com/vercel-labs/skills/blob/main/src/agents.ts
- 当前有多个 agent 共用 `.agents/skills`，例如 `opencode`、`codex`、`cursor`、`gemini-cli`、`github-copilot`、`kilo`、`kimi-code-cli`、`pi`、`warp`、`zed`；另有 `amp`、`cline`、`droid` 等也使用该值，不应根据旧印象推断专属目录。  
  原始源码：https://github.com/vercel-labs/skills/blob/main/src/agents.ts
- 专属目录并不总等于 `.<name>/skills`：例如 `codearts-agent` 为 `.codeartsdoer/skills`，`forgecode` 为 `.forge/skills`，`tabnine-cli` 为 `.tabnine/agent/skills`，`zenflow` 为 `.zencoder/skills`，`eve` 为 `agent/skills`。  
  原始源码：https://github.com/vercel-labs/skills/blob/main/src/agents.ts
- 当前源码没有 DeepSeek Harness / `dsh` 条目；本项目设计中的 `.dsh/skills` 属于本项目自有补充，不能声称来自该上游表。  
  原始源码：https://github.com/vercel-labs/skills/blob/main/src/agents.ts

### 2. 维护方式与稳定性

**事实**

- 表内路径靠源码修改维护，并伴随辅助函数、环境变量、平台分支与注释。例如 Reasonix 的全局目录分 Windows / 非 Windows，Zed 检测覆盖 XDG、APPDATA、Flatpak；这些是上游安装检测逻辑，不是项目相对 `skillsDir` 的新字段需求。  
  原始源码：https://github.com/vercel-labs/skills/blob/main/src/agents.ts
- 文件提交历史显示维护者会通过独立提交直接修正表项：2026-10-30 调整 Reasonix 全局 home，2026-10-09 调整 fx 的共享目录展示，2026-09-28 将 Pi 从 `.pi/skills` 改为通用 `.agents/skills`。也就是说上游表是当前事实来源，但不是冻结规范。  
  提交记录：https://github.com/vercel-labs/skills/commit/7ae8e2c63a163fcade6dd36f4223d37befa9b6f9  
  提交记录：https://github.com/vercel-labs/skills/commit/cc5fb50a1b3235bc30c330fa03471d5b2c69363b  
  提交记录：https://github.com/vercel-labs/skills/commit/2a286c7281e4c94bba8f3632bb1cbffab9a38daf
- 本次读取到的 `main` 文件 blob SHA 为 `76698bf3e764d8e8dad29d5ffd2ba5a754fd88cd`，路径最新提交为上述 2026-10-30 提交；后续同步应重新读取，不应把本笔记当长期缓存。  
  原始 API：https://api.github.com/repos/vercel-labs/skills/contents/src/agents.ts?ref=main  
  路径历史：https://api.github.com/repos/vercel-labs/skills/commits?path=src/agents.ts&per_page=5

### 3. 面向 M4 的建议（非完整方案）

- 同步时只抽取 `name` / `displayName` / `skillsDir`，不要移植 `globalSkillsDir`、安装检测和上游 UI 标志；本项目已明确 agent 是数据、落点相对项目根。
- 将本项目自有的 `agents`（通用位）与 `dsh` 明确标为自有条目，避免和上游 `universal` 混淆。
- 每次发布同步记录上游 commit/blob 标识并复核实际目录值；共享 `.agents/skills` 与不规则专属目录都应按表处理，不添加命名推断规则。
- 2026-11-03 实施 M4 时从 CDN 重新读取 `main` 源码，解析出 79 个上游条目的名称、显示名与项目目录；文件内容哈希仍为 `76698bf3e764d8e8dad29d5ffd2ba5a754fd88cd`，与最早读取一致。内置表另补自有 `agents` 与 `dsh`，共 81 项。

## 三、三句核心结论

1. `git2` 0.21.0 默认 features 不含 HTTPS/SSH，认证、push、fetch 都有底层 API，但私有库网络能力取决于显式构建配置与实际发布验证。
2. git2 没有一个调用就完成安全 pull 的高级接口：fetch 后仍需处理分析、引用、index、工作树与冲突，且分支和全部版本 tag 的同步要分别核实。
3. 上游 agent 表是持续手改源码的静态注册表；M4 只需抽取 `name`、`displayName`、`skillsDir`，并自行补充 `agents` / `dsh`，不应照搬其全局路径、检测函数和 UI 字段。
