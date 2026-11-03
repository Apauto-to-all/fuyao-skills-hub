---
name: manage-skills
description: 用 fsh CLI 管理技能库：入库、发版、归档、彻底删除、恢复、体检。一切操作走 fsh 命令，不手改导出目录与登记簿。
---

# manage-skills：fsh 技能库操作指南

fsh 是 fuyao-skills-hub 的命令行：hub 里以 git tag 为版本真身，目录只是编辑面与按需导出。
本 skill 教你（AI）何时调哪条命令、怎么读输出、哪些事绝对不能做。

## 核心模型（30 秒版）

- **hub 根**：`--hub <dir>` 显式指定，缺省用 fsh.exe 同级 `data\`
- **入库**：`add` 把本地目录复制进 hub，frontmatter 的 `name` 就是键末段（须等于目录名），自动打首版 tag；`--prefix <路径>` 给键加组织前缀（如 `anthropics/skills`）
- **发版**：先在 SKILL.md frontmatter 写目标版本号（如 `version: "1.2.0"`，须带引号、三段数字、大于当前版本），`fsh release <skill>` 照号发版。发版把工作副本当前改动（含脏）一起打成新 tag——改完随时可发，不用先「保存」
- **归档**：`archive` 删工作副本与导出目录，版本 tag 与历史全留，`restore` 可取回
- **彻底删除**：`remove` 连全部 tag 一起删；被项目引用时 archive 与 remove 都会拒绝

## 命令选择表

| 场景 | 命令 |
| --- | --- |
| 初始化 hub | `fsh init [--hub <dir>]`（幂等） |
| 新技能要进库 | `fsh add <目录> [--prefix <路径>]`（目录须含带 frontmatter 的 SKILL.md） |
| 看库里有什么 | `fsh list [--tag <标签>]` |
| 看某技能来头与被谁引用 | `fsh show <skill>` |
| 技能改完了要发新版 | 在 frontmatter 写号 → `fsh release <skill>`（号须大于当前版本） |
| 对比两个版本差异 | `fsh diff <skill> <v1> <v2>` |
| 暂时不用但留着历史 | `fsh archive <skill>` |
| 确定永远不用 | `fsh remove <skill>`（连 tag 删；被引用会拒绝） |
| 归档的技能想拿回来编辑 | `fsh restore <skill>` |
| 清理无引用导出目录 | `fsh gc [--dry-run]` |
| 备份 / 体检 | `fsh backup <目录>` / `fsh doctor` |

## 读输出：--json 信封与退出码

全部命令支持 `--json`，stdout 一行：

```json
{"ok":true,"data":{…}}                          // 成功
{"ok":false,"error":{"code":"…","message":"…"}} // 失败（code 是稳定标识，如 already_exists / referenced）
```

退出码：`0` 成功 · `1` 错误（doctor 出 FAIL 也是 1）。
建议习惯：读输出用 `--json`；第一次跑写命令先 `--dry-run` 看动作清单。

## 项目管理（`projects.toml`，ignored）

技能的用法只记在 **hub 侧**的 `projects.toml`，项目目录里不放 fsh 的任何文件：

```toml
[[projects]]
path = 'E:\proj\demo'                 # 项目根，绝对路径
last_used = 1762301234                # 最后一次被 fsh 操作的时间

[[projects.skills]]
key = "manage-skills"                 # hub 里的唯一键（store 内相对路径，可多级）
version = "0.1.0"                     # 可选：不写 = 跟最新
agents = ["agents", "opencode"]       # 铺给哪些 agent
link = "junction"                     # 实际采用的方式：symlink | junction | copy
```

落点由 agent 的 `skills_dir` + 键末段现算；登记簿由 fsh 机器写，不要手改。

## 红线（违反即破坏版本真身）

1. **禁止手改 `export\`**：导出目录是从 tag 派生的，手改后内容与 tag 不符，gc 会判脏
2. **删除一律走 fsh**：先 `archive` 保历史，确定不要了再 `remove`
3. **不手写登记簿**：`projects.toml` 归 fsh 维护
4. 写操作并发不必担心：fsh 内部有全局文件锁，两个 fsh 同时写不会打架
