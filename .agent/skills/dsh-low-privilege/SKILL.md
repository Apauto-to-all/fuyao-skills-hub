---
name: dsh-low-privilege
description: 处理 DSH 工作区的低权限问题。DSH 会限制工作区目录树的 Windows 权限（账户授权不足、低完整性标签），引发各种"拒绝访问"类故障：工作区内编译或运行的程序报 os error 5 / 0x800700AA、GUI 应用初始化即崩、程序在工作区外正常而在工作区内异常等。内含成因、诊断与修复
---

# DSH 工作区低权限问题

## 成因：两层限制

DSH 对其工作区目录树施加两层 Windows 权限限制，两层独立、需分别诊断：

1. **授权层（ACL）**：用户账户对部分目录缺少完全控制（无 WRITE_DAC / WRITE_OWNER 条目），导致改权限、改标签这类操作本身被拒
2. **完整性层（标签）**：整棵树被打上低完整性标签（`Mandatory Label\Low Mandatory Level`）。树内生成的文件（含编译产物）继承标签；从带标签的可执行文件启动的进程以低完整性运行，被 Windows 完整性机制（MIC）禁止写任何中完整性对象——工作区外几乎一切（`%LOCALAPPDATA%`、`%APPDATA%` 等）都是中完整性

## 低权限可能出现的症状

| 症状 | 层 | 机制 |
| --- | --- | --- |
| `拒绝访问。 (os error 5)` | 标签层或授权层 | 新建/打开中完整性资源被拒，或账户缺授权 |
| `请求的资源在使用中。 (HRESULT 0x800700AA)` | 标签层 | 对已存在的中完整性资源拿写锁失败 |
| GUI 应用（WebView2/Electron 类）启动即崩 | 标签层 | 最常见受害场景：初始化要写 `%LOCALAPPDATA%` |
| 同一程序工作区外正常、工作区内异常 | 标签层 | 判别性最强的特征 |
| DSH 会话内工具执行被拒 | 沙箱会话限制 | 属预期行为，不在本技能修复范围 |

## 诊断

对工作区内路径执行：

```powershell
icacls <路径>                                # 看授权条目：本账户是否有 (F)
icacls <路径> | Select-String Mandatory    # 看完整性标签
```

- 显示 `Low Mandatory Level` → 标签层问题 → 执行下面修复
- 本账户无 (F) 条目 → 授权层问题 → 先补授权（下面第 1 步，或用 DSH 的 diagnose-windows-sandbox-acl 修复工具，它只修授权、不动标签）

## 修复（用户自己的 PowerShell 执行，无需管理员）

改标签需要 WRITE_OWNER 权限，默认的 Modify 不够，必须先补全控制。对需要的项目子树执行两步：

```powershell
# 1. 补全控制（文件所有者可自我授权）
icacls "<项目子树>" /grant "<用户名>:(OI)(CI)F" /t /c /q

# 2. 标签改为 medium；此后子树内新生成的文件都继承 medium
icacls "<项目子树>" /setintegritylevel medium /t /c /q
```

node_modules（pnpm 符号链接结构）等会有一批"拒绝访问"失败，无碍：改不了标签的文件只被进程读取、不作为进程启动，忽略即可。

完成标准：两步在 node_modules 之外全部成功；目标可执行文件 icacls 显示 `Medium Mandatory Level`；原失败操作恢复正常。

## 注意

- 只改需要的项目子树，工作区其余目录保持原样
- DSH 可能在后续会话重新给工作区打标签、重置授权，症状复发时重跑两步
- 授权层与标签层是两个独立问题，修了一层还报错就查另一层
- 被低完整性进程污染过的数据目录（如 `%LOCALAPPDATA%\<应用标识>`），删除后由应用重建
