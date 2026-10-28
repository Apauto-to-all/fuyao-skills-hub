# 域文档

工程技能在探索本仓库代码时，应如何读取本仓库的领域文档。

## 探索前先读这些

- 仓库根目录的 **`GLOSSARY.md`**
- **`docs/adr/`**：读与你要动的区域相关的 ADR

本仓库的布局是 **single-context**（单上下文）：全仓库共用一套用语，术语表只有根目录这一份 `GLOSSARY.md`，架构决策统一放根目录 `docs/adr/`；不存在 `GLOSSARY-MAP.md`，也没有按子项目分置的术语表。

这些文件不存在时**静默继续**：不要指出它们缺失，也不要提前建议创建。`/domain-modeling` 技能（可经 `/grill-with-docs` 与 `/improve-codebase-architecture` 到达）会在术语或决策真正敲定时惰性创建它们。

## 文件结构

```text
/
├── GLOSSARY.md          ← 全仓库唯一术语表
├── docs/
│   ├── adr/             ← 架构决策记录
│   └── agents/          ← 技能配置（本目录）
├── crates/              ← Rust workspace 成员
├── src-tauri/           ← 桌面端（Tauri）
└── src/                 ← 前端
```

## 用术语表里的词

输出里出现领域概念时（issue 标题、重构提案、假设、测试名），一律使用 `GLOSSARY.md` 定义的那个词，不要漂到术语表明确不用的同义词上。

需要的概念还没进术语表，是一个信号：要么你在发明项目不用的语言（重新考虑），要么确实有缺口（记下来交给 `/domain-modeling`）。

## ADR 冲突要指出

如果你的输出与既有 ADR 相矛盾，显式指出来，不要默默覆盖：

> _与 ADR-00XX 冲突，但值得重新讨论，因为……_
