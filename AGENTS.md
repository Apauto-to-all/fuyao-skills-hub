# fuyao-skills-hub 项目规范

项目是 agent 技能的本地中心库（`fsh` 命令行 + `fsh-core`）：技能只存一份在本地 hub（一个 git 仓库），按项目或全局以目录链接铺进各 AI 工具的技能目录；界面给人看，命令行给 AI 用。

## 分派与执行

主代理是队长（派活、盯进度），子代理是队员（干活）。

- **三种方式**：自己做（活小或上下文连贯）／串行（清晰、量大，不建树）／并行（互相独立，一任务一员一树）。
- **各自编译**：并行时每棵树各持一份编译产物，落在项目自带 `target`——共享会让编译复用别的树的产物，给出假绿。
- **派活**：一人一任务，3 席并发，验完即补；派单第一步是切进自己的树、`pwd` 自检（队员默认继承队长的目录）。
- **队员**：活自己做完——二级子代理的通知传不到他手上。

## 阶段与文档

- **CLI + core 优先，GUI 冻结**：`src/`、`src-tauri/` 不进构建图，不编译、不测试、不审查；只在用户要求时动，其余时间连提都不提。
- **文档分工**：`docs/设计文档.md` 只写已交付功能；未实现的写 `docs/未来计划.md`，动工时整段删除。

## 需求入口

需求先过讨论，再动代码，按目标清晰度选一条：

- **流程 A（目标清晰）**：讨论对齐 → **用户手动** 收成规格 → **用户手动** 拆票 → 逐票实现。
  - 对齐 = 验收面能写成一句可测的话，术语与决策已记下。
  - 拆票 = 按模块组切，一张票带一处可观察的行为变化；同一改动面留在同一张票。
  - 实现 = 票的验收面通过，质量门绿。
- **流程 B（大而模糊）**：先建共享地图、逐票推到迷雾散尽 → 同样由用户手动收规格、拆票 → 逐票实现。

两个手动步只提醒、不代做。实施票全关后关规格票，并附逐项核对结论。

## 交付

一条分支、一个 PR、squash 落主干：

```bash
git switch -c <类型>/<主题>        # feat fix docs refactor chore ci
git push -u origin <类型>/<主题> && gh pr create
gh pr checks --watch --fail-fast   # 盯到出结果；退出码非 0 即失败
gh pr merge --squash --delete-branch
git switch main && git pull --ff-only && git fetch --prune
```

`main` 受保护：`check`（格式、clippy 含测试、全量测试）绿了才合得进去，管理员同等。一特性一个提交，实现、测试与文档一并落下。

**开 PR 时按改动性质打一个 label**，Release 正文靠它分组，不打就全落进「其他变更」：`feat`→`enhancement`，`fix`→`bug`，`docs`→`documentation`，`chore`/`refactor`/`build`/`ci`/`test`→`chore`。

**PR 开出后自己盯到出结果**：

- 拉一个后台任务盯，出结果后主动汇报。
- 推送之后先等 run 挂到 PR 上：`gh pr checks --watch` 在 run 尚未注册时会直接输出 `no checks reported` 并**退出码 0**——这是假绿，不是通过。用 `gh run list --branch <分支> --limit 1` 确认那次 run 存在且属于最新 commit，再开始盯。
- **绿了**：汇报「PR #N 通过」，把合并交回用户决定。用户决定合并后，删分支、切回 `main`、拉取、清理远端跟踪一并做完。
- **红了**：自己读 `gh run view <id> --log-failed` 定位真因，改代码、跑质量门确认修好，汇报真因与修复内容；征得同意后推送，推送后在该 PR 记一条评论说明真因。
- 红在**测试**上时，先问「这个用例为什么本地绿、CI 红」——环境差异（`CI` 变量、路径分隔符、大小写不敏感文件系统、时区）比逻辑错误更常见。

**授权点**：建分支、推送、提 PR、合并、tag、仓库设置前，先用问答工具征得同意，拿到**明确答复**再执行。

## 发布

升版走 PR，合入 main 后再打 tag：

```bash
# 升版 PR：改 [workspace.package].version，再同步 lock
cargo update --workspace
git add Cargo.toml Cargo.lock

# 合入 main 后打 tag
git tag -a v<版本> -m "<说明>" && git push origin v<版本>
```

`Cargo.lock` 里 `fsh-cli` 与 `fsh-core` 各自记着版本号，只改 `Cargo.toml` 不跑 `cargo update --workspace` 就会与工作区版本脱节，`--locked` 直接报错。

测试里引用「当前版本」一律用 `CURRENT_VERSION` 或 `env!("CARGO_PKG_VERSION")`，不写字面量；「比当前新」的构造值用恒高的 `9.9.9`。写死版本号的测试会在下一次升版时变红。

tag 与版本不一致即失败；Release 条目按 PR label 归类。

## 通用

- **注释**：只写当前代码——设计依据就地陈述，不标来源；删改原因写进提交消息。
- **临时文件**：一律写 `.tmp/`，用完即删。
- **工具**：要确认就在对话里问用户；派完活继续做不依赖它的部分，或停手等消息；已派出去的活留给队员做完。

## Rust

依赖先搜素最新版本，只写在根 `Cargo.toml`，版本只写主次，子 crate 用工作区版本；动手前加载 rust-patterns 与 rust-testing。

- **质量门**：格式化、clippy（连测试代码一起）、全量测试全绿才算交付——只查生产代码会漏掉测试里的问题。
- **模块形状**：一个文件一个概念，不止一个就拆目录模块；文件上限约 500 行；测试住在被测代码旁边。

## 指针

- **提交消息**：写提交消息前加载 `git-commit` 技能——类型、范围、标题与正文的写法在那里，不凭印象模仿既有提交。
- **日志**：写日志前读 `docs/日志规范.md`。
- **票**：读或写 `.scratch/<feature>/` 下的规格与实施票前，格式见 `docs/agents/issue-tracker.md`。
- **分诊标签**：给票打分诊标签、或读票上标签前，角色到字符串的映射见 `docs/agents/triage-labels.md`。
- **领域**：探索代码前先读 `GLOSSARY.md` 与相关 `docs/adr/`；单上下文布局见 `docs/agents/domain.md`。
