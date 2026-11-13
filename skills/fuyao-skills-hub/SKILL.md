---
name: fuyao-skills-hub
description: 用 fsh CLI 管理技能库：入库、列表、归档、恢复、彻底删除、铺给项目或全局、撤回、检索与体检。一切操作走 fsh 命令，不手改归档目录与登记簿。
metadata:
  version: "0.1.0"
  source: Apauto-to-all/fuyao-skills-hub
  tags: [fsh, 技能库]
---

# fuyao-skills-hub：fsh 技能库操作指南

fsh 是 fuyao-skills-hub 的命令行：hub 的 git 是自动审计日志（main 单分支、无 tag），
每次写命令落盘成功后自动产生一条机器提交；`store\` 里的工作副本是技能唯一真身，
`archive\` 是与它平级同构的归档面。本 skill 教你（AI）何时调哪条命令、怎么读输出、
哪些事绝对不能做。

## 核心模型（30 秒版）

- **hub 根**：`--hub <dir>` 显式指定 > 环境变量 `FSH_HUB` > 缺省 `~\.fuyao-skills-hub`
- **入库**：`add` 把本地目录复制进 hub，frontmatter 的 `name` 就是键末段（须等于目录名）；`--prefix <路径>` 给键加组织前缀（如 `mattpocock/skills`）
- **frontmatter 形状**：`name`（小写字母数字连字符、≤64 字符）与 `description`（非空、≤1024 字符）必填；`source` 写在 `metadata` 下；其余字段（含 `metadata.version`）fsh 不解析、随内容透传
- **唯一键写法**：多级键一律 `/` 分段（如 `mattpocock/skills/tdd`）；反斜杠、空段、`.`/`..`、盘符冒号直接报 `invalid_skill_key`，不做归一或猜测
- **归档**：`archive` 把工作副本整目录移入 `archive\<键>\`（与 `store\` 平级、同构、tracked 进 git），`restore` 整目录移回
- **彻底删除**：`remove` 把 store 与 archive 里的副本都删掉；被项目或全局用法引用时 archive 与 remove 都会拒绝
- **自动提交**：写命令成功落盘后 fsh 自动提交一次（固定提交者 `fsh`，消息形如 `use(claude,cursor): tdd -> myproj`、`add(store): tdd`，只含相对标识）；提交失败只告警，不影响已落盘结果

## 命令选择表（10 个动词）

| 场景 | 命令 |
| --- | --- |
| 初始化 hub | `fsh init [-n] [--hub <dir>]`（幂等） |
| 新技能要进库 | `fsh add <目录> [--prefix <路径>] [-n]`（目录须含带 frontmatter 的 SKILL.md） |
| 浏览库内技能 | `fsh list [过滤词] [--archived]`，可写 `fsh ls`（键、description、source、使用清单；旗标切到归档面） |
| 暂时不用但留着 | `fsh archive <skill> [-n]`（整目录移入 `archive\`） |
| 归档的想拿回来 | `fsh restore <skill> [-n]`（移回 `store\`） |
| 确定永远不用 | `fsh remove <skill> [-n]`，可写 `fsh rm`（store 与 archive 一起删；被引用会拒绝） |
| 把技能铺给某项目 | `fsh use <skill> -a <a>[,<b>] -p <目录> [-n]`（在场 + 无登记 + 落点空闲） |
| 把技能铺给全局技能目录 | `fsh use <skill> -a <a>[,<b>] -g [-n]` |
| 撤回某目标里的某技能 | `fsh unuse <skill> -a <a>[,<b>] (-g \| -p <目录>…) [-f] [-n]`（`-a` 必填：点名要撤的 agent，名单点满整条记录即整条撤回） |
| 检索 hub 内文件内容 | `fsh search <模式>`（覆盖 store 与 archive；`-g` 在本命令是 glob，可重复） |
| 体检 | `fsh doctor`（装配、链接、死记录、孤儿落点与孤儿目录；只报不动） |

短名与长名等价、可混用：`-a`/`--agent`、`-p`/`--project`、`-g`/`--global`（`use`/`unuse` 的 global）、`-f`/`--force`、`-n`/`--dry-run`。`search` 的 `-g` 是 glob，两者分属不同命令。`--json` 只有长选项，没有 `-j`；不做前缀缩写。

## 读输出：--json 信封与退出码

全部命令支持 `--json`，stdout 一行：

```json
{"ok":true,"data":{…}}                          // 成功
{"ok":false,"error":{"code":"…","message":"…"}} // 失败（code 是稳定标识，如 already_exists / referenced / conflict）
```

退出码：`0` 成功 · `1` 错误（doctor 出 FAIL 也是 1）。
建议习惯：读输出用 `--json`；第一次跑写命令先 `-n`/`--dry-run` 看动作清单。

## 使用与撤回（`use` / `unuse`）

- **目标必须显式给**：`-g`/`--global`（至多一个）与 `-p`/`--project <目录>`（可重复）至少给一个；两者都不给报 `no_target`，要用当前目录必须写 `-p .`。
- **铺装**：`fsh use <skill> -a <a>[,<b>] -p <目录>`。落点一律直连 `store\` 工作副本。执行前一次性预检：技能在场、目标里没这条记录、全部落点空闲；任何一项不通过就停下，磁盘一个字不动。中途失败会回滚本次新增的落点。
- **全局使用**：把 `-p <目录>` 换成 `-g`，落点落进各 agent 的 `global_skills_dir`，记录挂在登记簿全局段；全局与项目记录互不覆盖，也可以 `-g -p <目录>` 一次把两处都铺。
- **落点**由 agent 的 `skills_dir`（项目）或 `global_skills_dir`（全局）+ 技能键末段现算，多个 agent 共用一个目录时只建一次入口；同一次使用必须同一种链接方式。
- **重铺**：先 `unuse` 再 `use`——记录还在时 `use` 会拒绝。
- **`-a`/`--agent` 必填且两边同形**：`use` 与 `unuse` 的 agent 名单都是必填，拼法一致（逗号分隔、可重复）；取值来自内置名单或 hub 里的 `agents\<键>.toml`（见下表），写不在名单里的键报 `unknown_agent`。
- **撤回**：`fsh unuse <skill> -a <a>[,<b>] -p <目录>`（或 `-g`），只撤点名 agent 的落点并从记录里摘掉它们，记录里没有别的 agent 时连记录一起删——名单点满整条记录即整条撤回。共用落点留到最后一个引用它的 agent 也被撤走时才删。名单里出现记录没有的 agent 报 `agent_not_registered`，整次不动盘。
- **多目标撤回**：一次调用给多个目标时，名单必须命中每个目标里已登记的 agent；有一个目标对不上就整批不动盘。删除前先检查全部落点：copy 被改过默认整体拒绝（`-f`/`--force` 只覆盖这一项），链接类型或目标与登记不符始终拒绝；缺失落点视为已撤。
- 被项目或全局用法引用着的技能不能 `archive` / `remove`，先 `unuse`。

## 用法登记簿（`projects.toml`，ignored）

技能的用法只记在 **hub 侧**的 `projects.toml`，项目目录里不放 fsh 的任何文件。项目身份是规范化绝对路径，写成表键——Windows 下是 `canonicalize()` 的原样形态（带 `\\?\` 扩展前缀、反斜杠、保留大小写，例如 `\\?\E:\work\demo`），不是正斜杠小写那种落点身份；全局只有一个作用域：

```toml
[projects."<规范化绝对路径>"]          # 表键 = 项目身份：canonicalize() 后的原样路径
last_used = 1762301234                # 最后一次被 fsh 操作的时间

[[projects."<规范化绝对路径>".skills]]
key = "builtin/fuyao-skills-hub"      # hub 里的唯一键（store 内相对路径，可多级）
agents = ["universal", "opencode"]    # 铺给哪些 agent
link = "junction"                     # 实际采用的方式：symlink | junction | copy

[[globals.skills]]                    # 全局用法挂全局段，没有项目身份层
key = "builtin/fuyao-skills-hub"
agents = ["dsh"]
link = "junction"
```

落点由 agent 的 `skills_dir` / `global_skills_dir` + 键末段现算；登记簿由 fsh 机器写，不要手改。

## agent 配置

`--agent` 的可用取值 = 随程序发布的内置名单 ∪ hub 里 `agents\<键>.toml` 定义的条目，文件名（去 `.toml`）就是键。想改某个内置项或加新 agent，就在 hub 的 `agents\<键>.toml` 放一个文件：

```toml
display_name = "My Agent"                 # 可选，缺省用文件名
skills_dir = ".my-agent/skills"           # 可选：项目落点，相对项目根，禁绝对路径与 ..
global_skills_dir = "~/.my-agent/skills"  # 可选：全局落点，可写 ~ 前缀、环境变量或绝对路径
```

`skills_dir` 与 `global_skills_dir` 至少给一个：只给项目目录的 agent 只能项目使用，只给全局目录的只能全局使用；全局目录的可移植写法在解析落点时展开成绝对路径，落点永远指向绝对路径。
同名文件整项覆盖内置项（不做字段合并），新文件名新增一个 agent；配置非法直接报错，不会回退内置项。

## 内置 agent 表

内置名单随程序发布，完整条目（`--agent` 取值、显示名、项目技能目录、全局技能目录、全局根适配）见 [references/agents.md](references/agents.md)。

## 红线

1. **归档与恢复只走 fsh**：`archive\` 是 tracked 的归档面，手挪目录会与登记、git 历史脱节
2. **删除一律走 fsh**：先 `archive` 收起来，确定不要了再 `remove`
3. **不手写登记簿**：`projects.toml` 归 fsh 维护
4. 写操作并发不必担心：fsh 内部有全局文件锁，两个 fsh 同时写不会打架
