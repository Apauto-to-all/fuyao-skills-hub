# fuyao-skills-hub

AI 技能中枢，集中管理、按项目链接、git 版本化

## 用法示例

`<skill>` 一律写完整键（`store\` 内的相对路径），多级键要写全。

```bash
# 只铺项目：目标必须显式给，不写 --project 就等于没给目标
fsh use mattpocock/skills/tdd --agent dsh --project D:/work/alpha

# 只铺全局：一台机器一份，与任何项目无关
fsh use fuyao-skills-hub --agent dsh --global

# 一次铺给全局与多个项目：全部目标一起预检，任一不合格整次不动盘
fsh use fuyao-skills-hub --agent dsh --global --project D:/work/alpha --project D:/work/beta

# 撤回：目标集合同样显式；只撤全局，项目一条不动
fsh unuse fuyao-skills-hub --global
fsh unuse fuyao-skills-hub --project D:/work/alpha

# 换版本：只改已有记录；不给版本即跟随最新，一次可带全局与多个项目
fsh upgrade mattpocock/skills/tdd@0.2.0 --project D:/work/alpha
fsh upgrade mattpocock/skills/tdd --project D:/work/alpha

# 批量升级：范围缺省是全 hub，只升「固定版本且落后」的记录
fsh upgrade --all --dry-run

# 先用 --dry-run 看动作清单，再加 --json 让程序读
fsh use fuyao-skills-hub --agent dsh --global --dry-run
```

- `--project` 没有缺省：要用当前目录必须写 `--project .`；`--project` 与 `--global` 都不给会报 `no_target`。
- 全局用法与项目用法互不覆盖：同一个技能可以既有全局记录，又有多个项目记录。
- 换版本走 `fsh upgrade`：`use` 只新建，撞上已登记会报 `already_registered` 并指向它。
