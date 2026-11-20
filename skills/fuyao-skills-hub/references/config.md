# 配置

`fsh -h` 已给出 hub 定位（`--hub` / `FSH_HUB` / 缺省目录），这里只写命令查不到的：文件里的字段，与 fsh 从不打印的运行时开关。

## hub.toml（tracked，人写）

库配置，随 hub 进 git。`fsh init` 建库时写出默认值，已存在的文件保持原样。

```toml
schema_version = 1

[update]
mode = "auto"
interval_hours = 24
registry = "https://registry.npmjs.org"
channel = "latest"
notify_in_ci = false
notify_when_piped = false
```

### [update]：版本检查

| 字段 | 缺省 | 作用 |
| --- | --- | --- |
| `mode` | `auto` | `auto` 每条命令末尾被动查；`manual` 只有 `fsh doctor` 查；`off` 完全不查 |
| `interval_hours` | `24` | 节流间隔；`0` 表示每条命令都查 |
| `registry` | `https://registry.npmjs.org` | 发布源根地址，换镜像改这里 |
| `channel` | `latest` | 哪个 dist-tag 算最新版 |
| `notify_in_ci` | `false` | `CI` 环境变量非空时是否也提示 |
| `notify_when_piped` | `false` | 输出被重定向（stderr 非终端）时是否也提示 |

六项都可省：缺整段，或只写想改的键，其余取缺省。关掉版本检查的开关是 `mode = "off"`。

`schema_version` 是这份文件自身的格式版本，fsh 从不校验它的值——原样留着。

## update.toml（ignored，机器写）

版本检查状态，被 `.gitignore` 的 `/*` 挡在仓库外：

```toml
last_check = 1795182451   # 上次检查时间（Unix 秒）
latest = "0.1.3"          # 上次查到的最新版本，查不到时缺席
```

fsh 每次检查后重写。手改无意义：下一次检查覆盖它。

## agents/<名>.toml（tracked，人写）

agent 定义。同名文件整项覆盖内置项，新文件名即新增；内置名单见 [agents.md](agents.md)。

```toml
display_name = "My Agent"                 # 可选
skills_dir = ".myagent/skills"            # 相对项目根，禁绝对路径与 `..`
global_skills_dir = "~/myagent/skills"    # 可写 `~` / `%VAR%` / `$VAR` / 绝对路径
```

`skills_dir` 给项目位、`global_skills_dir` 给全局位，至少给一个。

可移植写法只认三类：开头的裸 `~`（不认 `~user`）、环境变量（`%NAME%` 与 `$NAME` / `${NAME}`）、绝对路径。展开结果必须绝对且不含 `..`；变量未设置即报错，不会把变量名留成字面量。

## 运行时开关

| 开关 | 作用 |
| --- | --- |
| `RUST_LOG` | 日志级别。缺省文件层 `info`、终端层 `off`（终端只留命令自身输出）；设了则两层都跟随。日志写 hub 根的 `logs\`，按天滚动留 30 天 |
| `CI` | 非空即视为 CI 环境，版本检查默认不提示，见 `notify_in_ci` |
