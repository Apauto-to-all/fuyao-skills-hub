<p align="center">
  <img src="assets/icon.svg" width="96" height="96" alt="fsh">
</p>

<h1 align="center">fuyao-skills-hub</h1>

<p align="center">agent 技能的本地中心库：集中存放，按需铺装到项目或全局技能目录。</p>

<p align="center">
  <img alt="platform" src="https://img.shields.io/badge/platform-Windows%20x64-0078D6?logo=windows&logoColor=white">
  <img alt="rust" src="https://img.shields.io/badge/Rust-2024-CE422B?logo=rust&logoColor=white">
  <img alt="agents" src="https://img.shields.io/badge/agent-83-2F6FEB">
  <a href="LICENSE"><img alt="license" src="https://img.shields.io/badge/license-Apache--2.0-black"></a>
</p>

## 背景

各 AI 工具（下称 agent）的技能目录约定不同：Codex 为 `.agents/skills`，DSH 为 `.dsh/skills`，Qoder 为 `.qoder/skills`。同一技能需供多个 agent 使用时，通行做法是每个目录各存一份副本，内容一旦变更须逐份同步，副本之间也容易产生版本差异。

`fsh` 把全部技能集中存放在本地 hub（缺省 `~\.fuyao-skills-hub`，本身是一个 git 仓库）中，以 `fsh use` 铺进项目或全局的技能目录。落点是目录链接，指向 hub 内的工作副本，不生成副本；修改工作副本后，全部链接落点即时生效。写命令落盘后自动产生一次 git 提交，提交历史即操作记录。

`fsh list` 输出每个技能的唯一键、description 与使用情况（目标、链接方式、agent），无需逐个检查项目目录。

## 安装

需要 Rust 工具链，见 [rustup.rs](https://rustup.rs)。当前仅支持 Windows（win32-x64）。

```bash
git clone https://github.com/Apauto-to-all/fuyao-skills-hub.git
cd fuyao-skills-hub
cargo install --path crates/fsh-cli
```

## 基本用法

```bash
fsh init
fsh add D:/downloads/prompt-review --prefix acme
fsh use --prefix acme --skill prompt-review -a codex -a dsh -p D:/work/alpha
```

`fsh init` 建立 hub，重复执行只补齐缺失内容。`fsh add` 把技能目录复制进 hub，并提交一次；它解析 `SKILL.md` 的 frontmatter，要求 `name` 与目录名一致、`description` 非空；fsh 不修改技能文件内容。入库支持成套：源目录自身含 `SKILL.md` 即单技能，否则只看直接子目录（跳过 `.` 开头的目录与 `node_modules`），`--all` 收下这一套、`--skill` 点名其中几个。`fsh use` 在项目的 `.agents/skills`、`.dsh/skills` 下建立链接，并将本次使用写入登记簿。

`fsh init` 同时把内置技能 `builtin/fuyao-skills-hub` 收入 hub。该技能说明 fsh 的全部命令与约束，铺装到全局后可直接供 AI 查阅：

```bash
fsh use --prefix builtin --skill fuyao-skills-hub -a dsh -g
```

`fsh list` 的输出格式如下：

```text
Store skills

builtin/fuyao-skills-hub
  Description: 用 fsh CLI 管理技能库：建库、入库、浏览、铺装、撤回、归档、恢复、重组、删除、检索与体检。

acme/prompt-review
  Description: 按项目规范逐条复核改动，输出可执行的修复清单
  Uses:        D:\work\alpha   junction   codex, dsh
```

## 命令

| 用途 | 命令 |
| --- | --- |
| 建库、对齐内置技能 | `fsh init` |
| 入库（单技能或整套） | `fsh add <目录> [--prefix <前缀>] [--skill <名>… \| --all]` |
| 浏览 | `fsh list [过滤词] [--prefix <前缀>]`（别名 `ls`，`--archived` 只列归档面） |
| 铺给项目或全局 | `fsh use --prefix <前缀> (--skill <名>… \| --all) -a <agent>[,<agent>] (-g \| -p <目录>…)` |
| 撤回 | `fsh unuse --prefix <前缀> (--skill <名>… \| --all) -a <agent>[,<agent>] (-g \| -p <目录>…)` |
| 归档、恢复 | `fsh archive --prefix <前缀> (--skill <名>… \| --all)`、`fsh restore --prefix <前缀> (--skill <名>… \| --all)` |
| 重组键前缀 | `fsh move --prefix <旧前缀> --skill <名> --to <新前缀>`（别名 `mv`，末段不变） |
| 彻底删除 | `fsh remove --prefix <前缀> (--skill <名>… \| --all)`（别名 `rm`，被引用时跳过并报告） |
| 检索库内文件 | `fsh search <正则> [范围]`（`-i -F -w -g -l -c -A/-B/-C`） |
| 体检 | `fsh doctor` |

吃技能键的动词统一用 `--prefix` + `--skill`（可重复）定位，`--all` 表示该前缀下的全部技能（含更深层）、须与 `--prefix` 同给；`--prefix` 省略即根，`--all` 与 `--skill` 互斥。`add` 的 `--all` 例外：范围由源目录划定，不带前缀即入到根下。

## 使用约束

- 技能入口由 `--prefix <前缀>` + `--skill <名>` 两段拼出（如 `--prefix acme --skill prompt-review` 即 `acme/prompt-review`）：技能名恒为一段，层级写进前缀；前缀省略即根。反斜杠、空段、`.`/`..` 报 `invalid_skill_key`。
- `-p` 无缺省值。当前目录须显式写 `-p .`；`-g` 与 `-p` 均未给出时报 `no_target`。
- `-g` 与 `-p` 可同时给出。全部目标统一预检，任一目标不合格则整次调用不落盘。
- 已登记、或落点已被占的技能不能重复 `use`：该技能进 `skipped`（`already_registered` / `destination_occupied`），不覆盖已有记录与文件；须先 `unuse` 再重新铺装。
- 写命令均支持 `-n`，输出待执行动作清单，不修改 hub 与项目数据。
- 短选项与长选项等价：`-a`/`--agent`、`-p`/`--project`、`-g`/`--global`、`-f`/`--force`、`-n`/`--dry-run`。
- 写操作对 hub 根下的 `.lock` 加锁，获取失败报 `lock_held`，不排队等待。

## 目录结构

```text
~\.fuyao-skills-hub\          # hub 是一个 git 仓库；根目录按 --hub > FSH_HUB > 缺省值 解析
├─ store\<键>\SKILL.md       # 技能工作副本，可修改、可归档、可恢复
├─ archive\<键>\SKILL.md     # 归档面，结构与 store 一致，同名即同键
├─ agents\<名>.toml          # agent 定义，同名覆盖内置项，新文件名即新增
├─ projects.toml             # 登记簿：各项目与全局的技能使用记录（不入库）
├─ hub.toml                  # 库配置
└─ logs\                     # 日志，按天滚动（不入库）
```

链接按 symlink、junction、copy 顺序降级，实际方式记入登记簿，可在 `fsh list` 的 `Uses` 一栏查看。前两种直连工作副本，内容始终与 hub 一致；copy 为快照，内容刷新须先 `unuse` 再重新铺装。

多个 agent 的落点重合时只建立一个入口，登记簿仍记录全部 agent；撤回按 agent 逐个进行，最后一个引用撤回后删除该入口。

## 支持的 agent

`-a` 接受内置名单中的键，共 83 个。完整键名与对应的技能目录见 [references/agents.md](skills/fuyao-skills-hub/references/agents.md)。

内置名单以数据形式提供：在 hub 的 `agents\` 下放置 `<键>.toml`，同名文件整项覆盖内置项，新文件名即新增 agent。`skills_dir`（相对项目根）与 `global_skills_dir`（支持 `~` 前缀、环境变量或绝对路径）至少提供一个：

```toml
# ~\.fuyao-skills-hub\agents\my-agent.toml
display_name = "My Agent"
skills_dir = ".myagent/skills"
global_skills_dir = "~/myagent/skills"
```

## 输出格式

`--json` 输出单行信封：成功为 `{"ok":true,"data":…}`，失败为 `{"ok":false,"error":{"code","message"}}`。`code` 为稳定标识，如 `invalid_skill_key`、`destination_occupied`、`already_registered`、`lock_held`。写命令的 `data` 是 `applied` 与 `skipped` 两个清单，`skipped` 每项带 `key`、`code`（情形码，如 `already_exists`、`conflict`、`referenced`）与 `message`；`applied` 为空而 `skipped` 非空时退出码为 1。退出码 0 表示成功，1 表示失败或这次其实什么都没做。

## 体检

`fsh doctor` 执行十项检查。环境项：hub 可写性、是否位于云同步路径、本机链接能力、装配完整性、内置技能是否已铺装到全局且与程序内嵌内容一致。数据项：登记簿中是否存在指向已删除项目的记录、已登记落点是否在位、store 与 archive 下是否存在孤儿目录、键末段与 frontmatter 的 `name` 是否一致。检查只报告不修改，存在 FAIL 项时退出码为 1。

## 常见问题

**是否支持 Windows 以外的平台？**

目前不支持。目录链接依赖 symlink 与 junction，路径与盘符规则同样按 Windows 实现。

**技能以复制还是链接的方式进入项目？**

链接。按 symlink、junction、copy 顺序降级，实际方式记入登记簿。前两种方式下，修改工作副本无需重新铺装。

**在项目内修改技能是否会影响 hub？**

链接落点直接指向 hub 中的工作副本，修改落点等同于修改工作副本。copy 落点被改动后，`unuse` 报 `modified_copy`，确认丢弃须加 `-f`。

**项目目录被删除后如何处理？**

落点随项目目录消失，登记簿中留下失效记录。`fsh doctor` 的 `dead_projects` 与 `orphan_destinations` 会报告，使用 `fsh unuse` 清理。

**是否需要预先安装 git？**

不需要。git 能力静态链接在可执行文件中，hub 仓库由 `fsh init` 建立，自动提交不依赖外部 git。

**同一技能能否同时用于全局与多个项目？**

可以。`fsh use --prefix acme --skill prompt-review -g -p D:/a -p D:/b` 一次完成，全局记录与项目记录互不覆盖。

**被引用的技能能否删除？**

不能。`archive` 与 `remove` 把仍被引用的技能放进 `skipped` 清单并报告（`referenced`），不删该技能，须先 `unuse`。

## 许可

[Apache-2.0](LICENSE)
