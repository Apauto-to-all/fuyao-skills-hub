---
name: dsh-low-privilege
description: 处理 DSH 工作区的低权限问题。DSH 给工作区目录树打低完整性标签，导致会话内写不进或删不掉文件、编译打包报 os error 5 / 0x800700AA、GUI 应用启动即崩。内含成因、判据与两套修复方案，适用于任意项目
---

# DSH 工作区低权限问题

## 成因：两层限制

1. **授权层（ACL）**：账户对目录缺完全控制（无 WRITE_OWNER），改权限、改标签本身就被拒
2. **完整性层（标签）**：目录被打上 `Low` / `Medium` 标签。低完整性进程只能写同等或更低标签的对象，写更高的一律被拒；新建文件继承**父目录**标签

会话 shell 的完整性随档位变化：受限档位（`read-only` / `workspace-write`）下是 **Low**，`danger-full-access` 下实测是 **Medium**。Low 会话写不进 Medium 对象；DSH 档位另有一层按路径的限制（受限档位下工作区外一律拒绝），与标签无关。

## 症状

| 症状 | 层 | 走哪个 |
| --- | --- | --- |
| 会话内写/删报 `Permission denied`，文件工具却能写 | 标签 | 方案 A |
| 编译、打包报 `拒绝访问 (os error 5)` / `0x800700AA` | 标签 | 方案 B |
| GUI 应用（WebView2 / Electron）启动即崩 | 标签 | 方案 B |
| 同一程序工作区外正常、工作区内异常 | 标签 | 方案 B |
| 子进程报 `spawn EPERM`（`execSync`、带管道的 spawn） | 沙箱档位 | 提权到 `danger-full-access`，改标签无效 |

## 判据

按**谁往里写**定标签，别整棵树一刀切：

- 源码、文档、配置 → **Low**：agent 要能写能删
- 产物目录（编译输出、打包输出）→ **Medium**：产物是 Low 时，启动的进程也是 Low，写不了 `%LOCALAPPDATA%` 等中完整性资源
- 打包产物（exe、dll、安装包）继承所在目录的标签：Low 产物双击无反应、启动即崩或功能异常，先查它自己的 `Mandatory Label`，不是程序本身的 bug

## 诊断

```powershell
$root = '<项目根>'
icacls $root | Select-String Mandatory      # Low / Medium
(Get-Acl $root).Owner                       # 所有者必须是当前账户
```

修复命令由**用户在自己的终端跑**（普通终端就是 Medium，无需管理员）最稳妥；`danger-full-access` 档位下 agent 也能跑（该档位会话是 Medium），受限档位下 agent 跑必失败——Low 改不动 Medium 对象的标签，报 `拒绝访问。 已成功处理 0 个文件`。agent 经 bash 调 icacls 时，MSYS 会把 `/T` 这类开关当作路径改写，报「无效参数」或咬掉参数，加 `MSYS2_ARG_CONV_EXCL='*'` 可屏蔽；在 PowerShell 里直接跑没有这个问题。

## 方案 A：会话内写不进 / 删不掉

```powershell
$root = '<项目根>'.TrimEnd('\')
$me   = "$env:USERDOMAIN\$env:USERNAME"
# 排除表写具体路径：顶层与嵌套（如 "$root\apps\web\node_modules"）都要列全
$skip = @(
  "$root\target", "$root\node_modules", "$root\.git",
  "$root\dist", "$root\build", "$root\out", "$root\.next", "$root\.venv", "$root\bin", "$root\obj"
) | Where-Object { Test-Path $_ }

# 每层分三类：①排除的（不碰）②跳过的（子树内含排除路径：/T 会误伤，只打自身再下钻重分类）③干净的（整块处理）
function Set-LowTree($dir) {
  $children = @(Get-ChildItem $dir -Force)
  $children | Where-Object { -not $_.PSIsContainer -and $skip -notcontains $_.FullName } | ForEach-Object {
    icacls $_.FullName /grant "${me}:F" /C /Q                    # 文件：裸 F + 裸 L（文件带不了继承位）
    icacls $_.FullName /setintegritylevel 'L' /C /Q
  }
  foreach ($c in @($children | Where-Object PSIsContainer)) {
    if ($skip -contains $c.FullName) { continue }                # ① 排除的：grant 与打标都不碰
    if ($skip | Where-Object { $_.StartsWith($c.FullName + '\', 'OrdinalIgnoreCase') }) {
      icacls $c.FullName /grant "${me}:(OI)(CI)F" /C /Q          # ② 跳过的：只打自身，不带 /T
      icacls $c.FullName /setintegritylevel '(OI)(CI)L' /C /Q
      Set-LowTree $c.FullName
    } else {
      icacls $c.FullName /grant "${me}:(OI)(CI)F" /T /C /Q        # ③ 干净的：授权整块
      icacls $c.FullName /setintegritylevel 'L' /T /C /Q          # 存量：裸 L 才能盖掉老文件的显式 Medium
      icacls $c.FullName /setintegritylevel '(OI)(CI)L' /T /C /Q  # 增量：补齐每层目录继承位，必须放在裸 L 之后
    }
  }
}
icacls $root /grant "${me}:(OI)(CI)F" /C /Q                      # 根自身（根永远按「跳过的」处理）
icacls $root /setintegritylevel '(OI)(CI)L' /C /Q
Set-LowTree $root
```

- 存量文件必须被命令碰到：旗标版 `(OI)(CI)X /T` 能改写**目录**（含内层），但改不动**带显式标签的文件**——显式 ACE 优先于继承，icacls 却照样报「已成功处理 N 个文件」；没有显式标签的文件会跟随父目录变成 Low。所以干净子树靠裸 `L /T` 覆盖存量，下钻层靠逐文件打覆盖
- 打标两条分工、顺序不能反：裸 `L` 管**存量**（写入普通 ACE，覆盖显式标签，同时会刷掉目录的继承位）；旗标版 `(OI)(CI)L` 管**增量**（目录继承位，以后新建文件自动 Low）。裸 L 会刷掉目录继承位，所以旗标版必须收尾；旗标版要带 `/T` 才能把继承位送到**内层**目录——不带 /T 只补顶层，内层目录下的新建文件拿不到标签，等同 Medium
- grant 必须先于 setintegritylevel、同对象成对执行：改标签需要 WRITE_OWNER，默认的 Modify 不够，不先补全控制则每条打标命令都被拒；排除目录对两者都零接触
- 「跳过的」判定是排除表具体路径的前缀比对（O(1)）；不要按名字扫子树，那会退化成全树枚举
- 只排除嵌套路径而不排除其父目录时（如只排除 `node_modules\.pnpm`），父目录其余子树会被整块打 Low，这是排除表的配置语义
- 排除表针对**目录**：把文件写进排除表没有实际效果，它照样继承父目录的标签（脚本只是不主动给它写显式标签）

## 方案 B：编译 / 打包 / 启动失败

```powershell
icacls '<产物目录>' /grant "${env:USERDOMAIN}\${env:USERNAME}:(OI)(CI)F" /T /C /Q
icacls '<产物目录>' /setintegritylevel '(OI)(CI)M' /T /C /Q
```

- 两条里的 `<产物目录>` 换成本仓库实际产物目录（如 `E:\proj\target`）
- 只对产物目录做；漏掉 `(OI)(CI)` 会导致清空产物后复发
- 彻底做法：产物目录外置，工作区整棵树留 Low（cargo 用 `[build] target-dir`，其他工具用各自的输出目录配置）

## 验证

```powershell
icacls <目录> | Select-String Mandatory
```

方案 A 后会话内能删文件；方案 B 后产物 `icacls` 显示 `Medium` 且程序能正常启动。

方案 A 跑完必须扫一遍残留：参照目录的标签看不出里面文件的情况，而带显式标签的文件不会被旗标版 `/T` 覆盖，漏网时 icacls 照样报成功。

```powershell
Get-ChildItem $root -Recurse -File -Force -ErrorAction SilentlyContinue |
  Where-Object { $_.FullName -notmatch '\\(node_modules|\.git|target)\\' } |
  ForEach-Object { if ((icacls $_.FullName) -match 'Medium') { $_.FullName } }
```

输出为空才算干净；有输出就对这些文件逐个跑裸 `icacls <文件> /setintegritylevel L /C /Q`。

## 注意

- 不要删 `S-1-4-*` 条目，不要删 `Everyone:(CI)(DENY)(DC)`——DSH 打的，删了破坏沙箱
- 不要改所有者，不要删现有权限条目，只做追加
- DSH 后续会话可能重置标签；产物目录被清空重建后会重新继承 Low，重跑方案 B
- 只改需要的子树，其余目录保持原样
