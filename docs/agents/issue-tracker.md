# Issue 跟踪器：GitHub

本仓库的 issue 与规格文档都存放在 GitHub Issues 里，所有操作一律使用 `gh` CLI。

## 约定

- **建 issue**：`gh issue create --title "..." --body "..."`；多行正文用 heredoc
- **读 issue**：`gh issue view <编号> --comments`，评论用 `jq` 过滤，同时取出标签
- **列 issue**：`gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'`，按需加 `--label` 与 `--state` 过滤
- **评论 issue**：`gh issue comment <编号> --body "..."`
- **打标签 / 摘标签**：`gh issue edit <编号> --add-label "..."` / `--remove-label "..."`
- **关闭 issue**：`gh issue close <编号> --comment "..."`

仓库由 `git remote -v` 推断；在克隆目录内执行时 `gh` 会自行识别。

## PR 是否作为需求入口

**PR 作为需求入口：否。**（若本仓库把外部 PR 当功能请求处理，把这里改成 `yes`；`/triage` 会读这个开关。）

设为 `yes` 时，PR 与 issue 走同一套标签与状态，改用 `gh pr` 系列的等价命令：

- **读 PR**：`gh pr view <编号> --comments`，要看改动用 `gh pr diff <编号>`
- **列出待分诊的外部 PR**：`gh pr list --state open --json number,title,body,labels,author,authorAssociation,comments`，只保留 `authorAssociation` 为 `CONTRIBUTOR`、`FIRST_TIME_CONTRIBUTOR`、`NONE` 的条目（去掉 `OWNER`/`MEMBER`/`COLLABORATOR`）
- **评论 / 打标签 / 关闭**：`gh pr comment`、`gh pr edit --add-label`/`--remove-label`、`gh pr close`

GitHub 的 issue 与 PR 共用一套编号，裸写的 `#42` 可能是两者之一：先 `gh pr view 42` 解析，失败再回退 `gh issue view 42`。

## 技能说“发布到 issue 跟踪器”时

建一个 GitHub issue。

## 技能说“取回相关票”时

执行 `gh issue view <编号> --comments`。

## 寻路（wayfinder）操作

供 `/wayfinder` 使用。**地图**是一个 issue，**子票**是挂在它下面的 issue。

- **地图**：一个带 `wayfinder:map` 标签的 issue，正文承载 Notes / Decisions-so-far / Fog。`gh issue create --label wayfinder:map`
- **子票**：以 GitHub sub-issue 形式挂到地图下（对 sub-issues 端点调 `gh api`）。仓库未启用 sub-issue 时，把子票加进地图正文的任务列表，并在子票正文顶部写 `Part of #<地图号>`。标签用 `wayfinder:<类型>`（`research`/`prototype`/`grilling`/`task`）。认领后把票指派给推进的人
- **阻塞**：用 GitHub 原生 issue 依赖，这是可见、规范的表达。加边：`gh api --method POST repos/<owner>/<repo>/issues/<子票号>/dependencies/blocked_by -F issue_id=<阻塞票的数据库 id>`，其中 `<阻塞票的数据库 id>` 取阻塞票的数字 **database id**（`gh api repos/<owner>/<repo>/issues/<编号> --jq .id`，**不是** `#编号`、也不是 `node_id`）。GitHub 用 `issue_dependencies_summary.blocked_by` 报告未解除的阻塞（只算还开着的，是实时的闸门）。依赖功能不可用时，回退成子票正文顶部的 `Blocked by: #<编号>, #<编号>` 行。所有阻塞票都关闭时，这张票解除阻塞
- **边界（frontier）查询**：列出地图下未关闭的子票（`gh issue list --state open`，范围限定为该地图的 sub-issue / 任务列表），剔除有未关闭阻塞的（`issue_dependencies_summary.blocked_by > 0`，或 `Blocked by` 行里还有开着的票）以及已有指派的；按地图顺序取第一个
- **认领**：`gh issue edit <编号> --add-assignee @me`，这是本次会话的第一次写操作
- **解决**：`gh issue comment <编号> --body "<答案>"`，随后 `gh issue close <编号>`，再把一条上下文指针（要点 + 链接）追加到地图的 Decisions-so-far
