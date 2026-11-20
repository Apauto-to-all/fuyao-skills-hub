---
name: fuyao-skills-hub
description: 用 fsh CLI 管理技能库：建库、入库、浏览、铺装、撤回、归档、恢复、重组、删除、检索与体检。选动词查「动词索引」。
metadata:
  version: "0.1.0"
  source: Apauto-to-all/fuyao-skills-hub
---

# fuyao-skills-hub：fsh 技能库操作指引

hub 是本地技能仓库：`store\` 放各技能的工作副本（技能真身），`archive\` 是平级同构的归档面，两者都 tracked 进 hub 自带的 git 审计日志（main 单分支）。技能铺进项目或全局后，落点直连 `store\` 工作副本；用法登记在 hub 侧的 `projects.toml`，由 fsh 机器读写。

## 工作约定

1. 生命周期与摆放走 fsh 子命令；内容更新直接改 store 副本（fsh 永不写 SKILL.md）。命令参数与用法细节先查 `fsh -h` 与 `fsh <子命令> -h`。
2. 写命令先带 `-n`（dry-run）看动作清单，确认后再去掉 `-n` 执行。
3. 程序化读输出用 `--json`：stdout 一行信封，成功 `{"ok":true,"data":{…}}`，失败 `{"ok":false,"error":{"code":"…","message":"…"}}`（code 是稳定标识，如 already_exists / referenced / conflict）；退出码 0 成功、1 失败，写命令 `applied` 空而 `skipped` 非空也是 1。

## 核心事实

- **技能键**：`store\` 内相对路径，一律 `/` 分段（如 `mattpocock/skills/tdd`）；反斜杠、空段、`.`/`..`、盘符冒号报 `invalid_skill_key`，改写键再试。
- **选择器**：吃技能键的动词统一用 `--prefix <前缀>`（可多段，省略即根）+ `--skill <名>…`（恒为一段、可重复，重复自动去重）或 `--all`（该前缀下的全部技能，须带前缀）。`--all` 与 `--skill` 互斥；`add` 例外，它的范围由源目录划定，`fsh add <目录> --all` 合法。预期内不适用的技能进 `skipped` 清单并报告，不堵其余。
- **frontmatter 契约**（add 校验）：`name`（小写字母数字连字符、≤64 字符）与 `description`（非空、≤1024 字符）必填，`name` 必须等于目录名。
- **remove 删两面**：store 与 archive 里的副本一起删。
- **自动提交**：改到 hub 的写命令落盘后 fsh 自动提交（固定提交者 `fsh`，消息只含相对标识）；落点与登记簿都不进 hub 仓库，`use`/`unuse` 因此没有提交；提交失败只告警，落盘结果不受影响。
- **升级提示**：fsh 每天最多查一次 npm 上的最新版，发现更新的版本就在 **stderr** 追加一行 `提示: fuyao-skills-hub 有新版本 <新>（当前 <当前>），升级: npm i -g fuyao-skills-hub@latest`。它不是错误，stdout 与退出码一律不变；`--json` 模式、输出被重定向、CI 里都不提示。要调间隔、换发布源或彻底关掉，改 `hub.toml` 的 `[update]` 表，字段见 [references/config.md](references/config.md)。

## 动词索引

| 意图 | 动作 |
| --- | --- |
| 建库或对齐内置技能 | `fsh init`（幂等，重复执行只补缺件） |
| 本地目录入库 | `fsh add <目录> [--prefix <前缀>] [--skill <名>… 或 --all]` |
| 浏览库内技能 | `fsh list [过滤词] [--prefix <前缀>]`，`--archived` 看归档面 |
| 检索文件内容 | `fsh search <模式>`（正则缺省，glob 收窄，覆盖 store 与 archive） |
| 铺给项目或全局 | `fsh use --prefix <前缀> --skill <名>… -a <a>[,<b>] (-p <目录>… 或 -g)`；整批用 `--all`；记录还在时先 `unuse` 再 `use` |
| 撤回铺装 | `fsh unuse --prefix <前缀> --skill <名>… -a <a>[,<b>] (-p <目录>… 或 -g)`；`-a` 点满整条记录即整条撤回 |
| 暂时收起 | `fsh archive --prefix <前缀> --skill <名>…`（或 `--all`）；被引用时进 skipped |
| 归档取回 | `fsh restore --prefix <前缀> --skill <名>…`（或 `--all`） |
| 重组键前缀 | `fsh move --prefix <旧前缀> --skill <名> --to <新前缀>`（别名 mv；末段不可变，改身份走整目录改名 + 重新 add） |
| 彻底删除 | `fsh remove --prefix <前缀> --skill <名>…`（或 `--all`）；被引用时进 skipped |
| 体检 | `fsh doctor`（十一项：装配、链接、内置技能一致性、死记录、孤儿落点与孤儿目录、程序版本是否有更新等；只报不动） |

`-a`/`--agent` 的合法取值 = 内置名单（完整条目见 [references/agents.md](references/agents.md)）∪ hub 里 `agents\<键>.toml` 定义的键。要新增或覆盖 agent，在 hub 的 `agents\` 放一个 TOML 文件，字段与可移植路径写法见 [references/config.md](references/config.md)；同名文件整项覆盖内置项，不做字段合并。
