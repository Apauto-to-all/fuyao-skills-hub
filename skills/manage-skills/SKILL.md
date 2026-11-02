---
name: manage-skills
description: 用 fsh CLI 管理技能库：入库、发版、分配到项目、查漂移、归档清理。一切操作走 fsh 命令，禁止直改 hub 文件。
---

# manage-skills：fsh 技能库操作指南

fsh 是 fuyao-skills-hub 的命令行：hub 里以 git tag 为版本真身，目录只是缓存。
本 skill 教你（AI）何时调哪条命令、怎么读输出、哪些事绝对不能做。

## 核心模型（30 秒版）

- **hub 根**：`--hub <dir>` 显式指定，缺省用 fsh.exe 同级 `data\`
- **入库**：`add` 把本地目录复制进 hub，frontmatter 的 `name` 就是键末段（须等于目录名），自动打首版 tag；`--prefix <路径>` 给键加组织前缀（如 `anthropics/skills`）
- **发版**：先在 SKILL.md frontmatter 写目标版本号（如 `version: "1.2.0"`，须带引号、三段数字、大于当前版本），`fsh version <skill>` 照号发版；不想手算就加 `--bump patch|minor|major` 让 fsh 代算号并写回文件。发版把工作副本当前改动（含脏）一起打成新 tag——改完随时可发，不用先「保存」
- **分配**：项目根写 `skills.toml` 声明要什么，`apply` 铺成链接（落点 `.fsh\skills\<键>`，保留键的嵌套结构）
- **归档**：`remove` 只删编辑面，版本 tag 与历史全留，项目供版不受影响；要彻底删用 `--purge`

## 命令选择表

| 场景 | 命令 |
| --- | --- |
| 新技能要进库 | `fsh add <目录> [--prefix <路径>]`（目录须含带 frontmatter 的 SKILL.md） |
| 看库里有什么 | `fsh list [--tag <标签>]` |
| 看某技能来头与被谁引用 | `fsh show <skill>` |
| 技能改完了要发新版 | 在 frontmatter 写号 → `fsh version <skill>`（号须大于当前版本）；或 `fsh version <skill> --bump patch\|minor\|major` 代算号 |
| 对比两个版本差异 | `fsh diff <skill> <v1> <v2>` |
| 暂时不用某技能 | `fsh remove <skill>`（归档，历史可溯） |
| 确定永远不用 | `fsh remove <skill> --purge`（连版本 tag 一起删；被项目引用会拒绝） |
| 归档的技能想拿回来编辑 | `fsh restore <skill>` |
| 项目要用技能 | 在项目根写 `skills.toml` → `fsh apply` |
| 改声明 / 升降级 / 回滚 | 改 `skills.toml` 里的版本号 → `fsh apply` |
| 检查项目链接是否漂移 | `fsh status`（退出码 2 = 有漂移，看 drift 列表） |
| 先看会做什么再动手 | 任何写命令加 `--dry-run` |
| 清理无引用缓存 | `fsh gc [--dry-run]` |
| 备份 / 体检 | `fsh backup <目录>` / `fsh doctor` |

## 读输出：--json 信封与退出码

全部命令支持 `--json`，stdout 一行：

```json
{"ok":true,"data":{…}}                          // 成功
{"ok":false,"error":{"code":"…","message":"…"}} // 失败（code 是稳定标识，如 already_exists / referenced）
```

退出码：`0` 成功 · `1` 错误（doctor 出 FAIL 也是 1）· `2` status 发现漂移。
`--json` 下退出码同样生效——先看退出码分支，再解析 data。

建议习惯：读输出用 `--json`；第一次跑写命令先 `--dry-run` 看动作清单。

## 项目侧 skills.toml 格式（人/AI 手写）

放项目根（不是 `.fsh\` 下），一条技能一个 `[[skills]]`：

```toml
[[skills]]
name = "manage-skills"   # hub 里的技能键（store 内相对路径，可多级）
version = "0.1.0"        # 精确 semver，或 "stable" 跟最新
target = "claude-code"   # 目标工具名（映射到具体目录属适配器阶段）
```

写完（或改完版本号）执行 `fsh apply`；账本 `skills.lock.toml` 与状态 `state.toml` 由 fsh 机器写，不要手改。

## 红线（违反即破坏版本真身）

1. **禁止直接改 hub 内任何文件**：`store\`、物化目录（`cache\<键>\<版本>\`）都是 git 管理或派生物，手改 = 漂移或历史污染
2. **改技能内容只在 hub 工作副本（`store\<键>\`）里改**，改完在 frontmatter 写号、`fsh version <skill>` 发版——但打开它也应该经由你的编辑流程，别绕过 fsh 做删除/改名
3. **删除一律走 `remove`**：默认归档保历史；`--purge` 前确认没有项目还在引用（被引用时 fsh 会拒绝）
4. **项目侧只手写 `skills.toml`**，lock/state 由 fsh 维护
5. 写操作并发不必担心：fsh 内部有全局文件锁，两个 fsh 同时写不会打架
