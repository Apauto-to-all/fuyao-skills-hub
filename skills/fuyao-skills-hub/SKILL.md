---
name: fuyao-skills-hub
description: 用 fsh CLI 管理技能库：入库、发版、归档、彻底删除、恢复、体检。一切操作走 fsh 命令，不手改导出目录与登记簿。
metadata:
  version: "0.1.0"
  source: Apauto-to-all/fuyao-skills-hub
  tags: "fsh, 技能库, 版本管理"
---

# fuyao-skills-hub：fsh 技能库操作指南

fsh 是 fuyao-skills-hub 的命令行：hub 里以 git tag 为版本真身，目录只是编辑面与按需导出。
本 skill 教你（AI）何时调哪条命令、怎么读输出、哪些事绝对不能做。

## 核心模型（30 秒版）

- **hub 根**：`--hub <dir>` 显式指定 > 环境变量 `FSH_HUB` > 缺省 `~\.fuyao-skills-hub`
- **入库**：`add` 把本地目录复制进 hub，frontmatter 的 `name` 就是键末段（须等于目录名），自动打首版 tag；`--prefix <路径>` 给键加组织前缀（如 `mattpocock/skills`）
- **frontmatter 形状**：顶层只放官方的 `name` / `description`（其余顶层字段原样透传）；fsh 自有的三条都住在 `metadata` 下——`version`（带引号的三段数字）、`source`、`tags`（一个逗号分隔的字符串）。把这三条平铺在顶层会被点名拒绝
- **唯一键写法**：多级键一律 `/` 分段（如 `mattpocock/skills/tdd`）；反斜杠、空段、`.`/`..`、盘符冒号与版本号形状直接报 `invalid_skill_key`，不做归一或猜测
- **发版**：先在 SKILL.md 的 `metadata` 下写目标版本号（`metadata` 块里 `version: "1.2.0"`，须带引号、三段数字、大于当前版本），`fsh release <skill>` 照号发版。发版把工作副本当前改动（含脏）一起打成新 tag——改完随时可发，不用先「保存」
- **归档**：`archive` 删工作副本与导出目录，版本 tag 与历史全留，`restore` 可取回
- **彻底删除**：`remove` 连全部 tag 一起删；被项目或全局用法引用时 archive 与 remove 都会拒绝

## 命令选择表

| 场景 | 命令 |
| --- | --- |
| 初始化 hub | `fsh init [--hub <dir>]`（幂等） |
| 新技能要进库 | `fsh add <目录> [--prefix <路径>]`（目录须含带 frontmatter 的 SKILL.md） |
| 看某技能来头与被谁引用 | `fsh show <skill>` |
| 技能改完了要发新版 | 在 `metadata` 下写号 → `fsh release <skill>`（号须大于当前版本） |
| 对比两个版本差异 | `fsh diff <skill> <v1> <v2>` |
| 暂时不用但留着历史 | `fsh archive <skill>` |
| 确定永远不用 | `fsh remove <skill>`（连 tag 删；被引用会拒绝） |
| 归档的技能想拿回来编辑 | `fsh restore <skill>` |
| 清理无引用导出目录 | `fsh gc [--dry-run]` |
| 把技能铺给某项目 | `fsh use <skill>[@<版本>] --agent <a>[,<b>] --project <目录>`（未归档 + 无登记 + 落点空闲） |
| 把技能铺给全局技能目录 | `fsh use <skill>[@<版本>] --agent <a>[,<b>] --global` |
| 撤回某目标里的某技能 | `fsh unuse <skill> [--agent <a>[,<b>]] (--global \| --project <目录>…) [--force]`（给 `--agent` 按 agent 撤，不给则全部 agent 一起撤） |
| 看有哪些 agent 可选 | `fsh agents`（每个 agent 的名字与来源，项目技能位与全局技能位按配置原文回显） |
| 看哪些项目在用技能 | `fsh projects`（项目、技能、落点是否存在） |
| 体检 | `fsh doctor` |

## 读输出：--json 信封与退出码

全部命令支持 `--json`，stdout 一行：

```json
{"ok":true,"data":{…}}                          // 成功
{"ok":false,"error":{"code":"…","message":"…"}} // 失败（code 是稳定标识，如 already_exists / referenced）
```

退出码：`0` 成功 · `1` 错误（doctor 出 FAIL 也是 1）。
建议习惯：读输出用 `--json`；第一次跑写命令先 `--dry-run` 看动作清单。

## 使用与撤回（`use` / `unuse`）

- **目标必须显式给**：`--global`（至多一个）与 `--project <目录>`（可重复）至少给一个；两者都不给报 `no_target`，要用当前目录必须写 `--project .`。
- **首次铺装**：`fsh use <skill>[@<版本>] --agent <a>[,<b>] --project <目录>`。不带 `@版本` 链接最新工作副本；带版本则从 tag 导出固定副本再链接。执行前一次性预检：技能未归档、目标里没这条记录、全部落点空闲；任何一项不通过就停下，磁盘一个字不动。中途失败会回滚本次新增的落点。
- **全局使用**：把 `--project <目录>` 换成 `--global`，落点落进各 agent 的 `global_skills_dir`，记录挂在登记簿全局段；全局与项目记录互不覆盖，也可以 `--global --project <目录>` 一次把两处都铺。
- **落点**由 agent 的 `skills_dir`（项目）或 `global_skills_dir`（全局）+ 技能键末段现算，多个 agent 共用一个目录时只建一次入口；同一次使用必须同一种链接方式。
- **升级 / 回滚**：先 `unuse` 再 `use <skill>@<版本>`——记录还在时 `use` 会拒绝。
- **整体撤回**：`fsh unuse <skill> --project <目录>` 或 `--global`，把该目标里这条技能的全部 agent 落点一起撤。删除前先检查全部落点：copy 被改过默认整体拒绝（`--force` 只覆盖这一项），链接类型或目标与登记不符始终拒绝；缺失落点视为已撤。
- **按 agent 撤回**：`fsh unuse <skill> --agent <a>[,<b>] --project <目录>`（或 `--global`），`--agent` 的拼法与 `use` 一致（逗号分隔、可重复），只撤这些 agent 的落点并从记录里摘掉它们；共用落点留到最后一个引用它的 agent 也被撤走时才删。名单里出现记录没有的 agent 报 `agent_not_registered`，整次不动盘。
- 被项目或全局用法引用着的技能不能 `archive` / `remove`，先 `unuse`。

## 用法登记簿（`projects.toml`，ignored）

技能的用法只记在 **hub 侧**的 `projects.toml`，项目目录里不放 fsh 的任何文件。项目身份是规范化绝对路径，写成表键——Windows 下是 `canonicalize()` 的原样形态（带 `\\?\` 扩展前缀、反斜杠、保留大小写，例如 `\\?\E:\work\demo`），不是正斜杠小写那种落点身份；全局只有一个作用域：

```toml
[projects."<规范化绝对路径>"]          # 表键 = 项目身份：canonicalize() 后的原样路径
last_used = 1762301234                # 最后一次被 fsh 操作的时间

[[projects."<规范化绝对路径>".skills]]
key = "builtin/fuyao-skills-hub"      # hub 里的唯一键（store 内相对路径，可多级）
version = "0.1.0"                     # 可选：不写 = 跟最新
agents = ["universal", "opencode"]    # 铺给哪些 agent
link = "junction"                     # 实际采用的方式：symlink | junction | copy

[[globals.skills]]                    # 全局用法挂全局段，没有项目身份层
key = "builtin/fuyao-skills-hub"
agents = ["dsh"]
link = "junction"
```

落点由 agent 的 `skills_dir` / `global_skills_dir` + 键末段现算；登记簿由 fsh 机器写，不要手改。

## agent 配置

`fsh agents` 列出认得的 agent 的配置（名字、显示名、项目目录与全局目录、来源；两个目录都按配置原文回显）。内置名单随程序发布；想改某个内置项或加新 agent，就在 hub 的 `agents\<键>.toml` 放一个文件：

```toml
display_name = "My Agent"                 # 可选，缺省用文件名
skills_dir = ".my-agent/skills"           # 可选：项目落点，相对项目根，禁绝对路径与 ..
global_skills_dir = "~/.my-agent/skills"  # 可选：全局落点，可写 ~ 前缀、环境变量或绝对路径
```

`skills_dir` 与 `global_skills_dir` 至少给一个：只给项目目录的 agent 只能项目使用，只给全局目录的只能全局使用；全局目录的可移植写法在解析落点时展开成绝对路径，落点永远指向绝对路径。
同名文件整项覆盖内置项（不做字段合并），新文件名新增一个 agent；配置非法直接报错，不会回退内置项。

## 红线（违反即破坏版本真身）

1. **禁止手改 `export\`**：导出目录是从 tag 派生的，手改后内容与 tag 不符，gc 会判脏
2. **删除一律走 fsh**：先 `archive` 保历史，确定不要了再 `remove`
3. **不手写登记簿**：`projects.toml` 归 fsh 维护
4. 写操作并发不必担心：fsh 内部有全局文件锁，两个 fsh 同时写不会打架
