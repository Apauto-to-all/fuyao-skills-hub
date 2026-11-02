# fuyao-skills-hub

技能（skill）的集中管理中心：以一个 hub 保存全部技能的唯一真身，按版本铺到各项目。本表是全库统一用语。

## Language

### 仓库面

**hub**：
中心库根目录，所有 skill 的唯一真身所在地。
_避用_：中心仓库、仓库根、data 目录

**store**：
hub 内的不可变版本仓库。
_避用_：技能库、存储区

**工作副本**：
`store\<相对路径>\` 下 tracked 的编辑面目录；可脏，永不成为链接目标。
_避用_：源目录、技能目录

**归档**：
删除 skill 的工作副本，仅保留 tag 与 git 历史；版本仍可被引用与物化，restore 可取回编辑面。与 `--purge`（连 tag 一并删除）相对。
_避用_：卸载、软删除

**物化**：
从 git tag 导出的实体目录（部署面）；缓存性质，可随时删 / 重建。
_避用_：导出、复制、安装目录

**GC**：
回收无引用物化目录等缓存对象；无损，可按字节重建。
_避用_：清理、purge

**顺手清**：
apply 等操作末尾顺带回收本次失引用的物化目录，不必等独立 GC。
_避用_：自动清理

**登记簿**：
hub 侧的全量项目名单（`projects.toml`），apply 成功即登记；GC 与 doctor 的引用查找账本。
_避用_：项目列表、注册表

**引用集**：
一次走查登记簿全部项目 lock 得出的 (skill, version) 引用对集合，附不可达事件与死记录；purge 守卫、GC 与顺手清共用的删除安全判定事实源。
_避用_：引用列表、谁在用

### 元数据面

**frontmatter**：
SKILL.md 顶部 YAML 块，AI 可编辑元数据的唯一住所；fsh 只用其中 name / description / version / source / tags 五字段，其余字段不解析、随内容透传。
_避用_：元数据文件、配置区

**version（发版输入）**：
frontmatter 顶层字段，AI 写目标版本号（SemVer `X.Y.Z`，须带引号）；fsh 校验其大于当前 tag 后 commit + tag + 物化。版本真身是 git tag，此字段是发版输入与展示镜像；写者是 AI，`--bump` 时由 fsh 代算代写。
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

**profile**：
项目的 skill 声明（`skills.toml`）。
_避用_：依赖清单、manifest

**lock**：
解析后的精确版本快照（`skills.lock.toml`）。
_避用_：lockfile、版本锁定文件

**adapter**：
某个 AI 工具/目录的适配配置。
_避用_：插件、集成

**链接**：
项目内指向物化目录的入口；按 symlink → junction → copy 三级降级建立。
_避用_：快捷方式、软链（统称）

**黑链接**：
绕过 fsh 手动创建、无 lock 登记的链接；悬空不算事故，可收编。
_避用_：野链接、非法链接

### 流程面

**deployment**：
一次「把某版本铺到某项目某工具」的记录。
_避用_：安装记录

**proposal**：
进化提案，未应用前不影响任何东西。
_避用_：草稿、变更请求

**漂移**：
实际状态与 lock 不一致。
_避用_：不一致、脏状态

**stable**：
技能的移动版本引用，始终指向最新版本 tag。
_避用_：latest、默认版本
