# fuyao-skills-hub

技能（skill）的集中管理中心：以一个 hub 保存全部技能的唯一真身，按需导出到各项目、各 agent 认的技能目录。本表是全库统一用语。

## Language

### 仓库面

**hub**：
中心库根目录，所有 skill 的唯一真身所在地。
_避用_：中心仓库、仓库根、data 目录

**store**：
hub 内的可编辑面根目录，`store\<键>\` 是技能的工作副本。
_避用_：技能库、存储区

**工作副本**：
`store\<相对路径>\` 下 tracked 的编辑面目录；可脏，可删可恢复；跟最新的项目直接链到它。
_避用_：源目录、技能目录

**归档**：
删除 skill 的工作副本与导出目录，仅保留 tag 与 git 历史；restore 可取回编辑面。
_避用_：卸载、软删除

**导出目录**：
`export\<键>\<版本>\`，从 git tag 树导出的实体目录；固定版本的项目链到它。可随时删、按字节重建。
_避用_：导出缓存、安装目录

**GC**：
回收无引用的导出目录与孤儿目录，清登记簿死记录；脏目录只报警不动手。无损，可按 tag 重建。
_避用_：清理

**登记簿**：
hub 侧的项目名单（`projects.toml`，ignored），用法的唯一记录；GC、archive、remove、show 判引用的唯一来源。
_避用_：项目列表、注册表、谁在用

### 元数据面

**frontmatter**：
SKILL.md 顶部 YAML 块，AI 可编辑元数据的唯一住所；fsh 只用其中 name / description / version / source / tags 五字段，其余字段不解析、随内容透传。
_避用_：元数据文件、配置区

**version（发版输入）**：
frontmatter 顶层字段，AI 写目标版本号（SemVer `X.Y.Z`，须带引号）；fsh 校验其大于当前 tag 后 commit + tag。版本真身是 git tag，此字段是发版输入与展示镜像；写者是 AI。
_避用_：版本号文件、版本记录

**source（来源）**：
frontmatter 顶层可选字段，外部 skill 套件的出处标识（推荐 `owner/repo` 风格，不校验）；自产 skill 不填。只做来源记录，不承担分组。
_避用_：分类、出处

**tags**：
frontmatter 顶层 YAML 列表（流式/块式等价）的自由标签，承担功能细分与过滤；建议 ≤5 个，不硬校验。
_避用_：子分类、keywords

**唯一键**：
skill 在 hub 中的身份：`store\` 内的相对路径（末段 = SKILL.md 所在目录 = frontmatter name）；tag 命名空间与项目侧引用均以它为键。目录在 store 内挪动即更换身份（等价重入库）。
_避用_：技能名（作键时）、路径 ID

### 项目面

**agent 配置**：
`agents\<名>.toml`，tracked 的 agent 定义（`display_name` 可选 / `skills_dir` 必填）；内置表在代码里，同名文件即覆盖，新文件名即新增。落点 = `skills_dir\<键末段>`。
_避用_：适配器、插件、集成

**项目使用**：
把某技能按某版本铺给某项目的某几个 agent，结果记进登记簿的一条技能记录（key / version / agents / link）。
_避用_：安装、安装记录

**链接**：
项目内指向工作副本或导出目录的入口；按 symlink → junction → copy 三级降级建立，实际采用的方式记在登记簿的 `link` 字段。
_避用_：快捷方式、软链（统称）、野链接

