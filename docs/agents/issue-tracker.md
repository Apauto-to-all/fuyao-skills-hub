# Issue 跟踪器：本地 Markdown

本仓库的 issue 与规格文档都以 Markdown 文件的形式存放在 `.scratch/` 下。

## 约定

- 一个特性一个目录：`.scratch/<feature-slug>/`
- 规格是 `.scratch/<feature-slug>/spec.md`
- 实施 issue 一票一个文件，放在 `.scratch/<feature-slug>/issues/<NN>-<slug>.md`，从 `01` 起编号，禁止合并成单个总票文件
- 分诊状态记在每个 issue 文件顶部附近的 `Status:` 行（角色字符串见 `triage-labels.md`）
- 评论与对话历史追加到文件底部，挂在 `## Comments` 标题下

## 技能说“发布到 issue 跟踪器”时

在 `.scratch/<feature-slug>/` 下新建一个文件（目录不存在就先建）。

## 技能说“取回相关票”时

读取所引路径上的文件；用户通常会直接给出路径或 issue 编号。

## 寻路（wayfinder）操作

供 `/wayfinder` 使用。**地图**是一个文件，**子票**是一票一个文件。

- **地图**：`.scratch/<effort>/map.md`（正文承载 Notes / Decisions-so-far / Fog）。
- **子票**：`.scratch/<effort>/issues/NN-<slug>.md`，从 `01` 起编号，正文写问题。`Type:` 行记录票的类型（`research`/`prototype`/`grilling`/`task`）；`Status:` 行记录 `claimed`/`resolved`。
- **阻塞**：文件顶部附近的 `Blocked by: NN, NN` 行。所列每个文件都 `resolved` 后，本票解除阻塞。
- **边界（frontier）**：扫描 `.scratch/<effort>/issues/`，找开放、未阻塞、未认领的文件；编号最小者先做。
- **认领**：先写 `Status: claimed` 并保存，再开始任何工作。
- **解决**：把答案追加到 `## Answer` 标题下，置 `Status: resolved`，再把一条上下文指针（要点 + 链接）追加到 `map.md` 中地图的 Decisions-so-far。
