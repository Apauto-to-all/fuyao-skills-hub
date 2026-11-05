# Tauri 2 桌面壳与 IPC：一手事实

- 调研日期：2026-11-05
- 对应设计：`docs/设计文档.md` §4 GUI 设计（四页），以及 §2.1「`src-tauri` 只做薄桥」
- 范围：command ↔ `invoke` 的映射与错误形状、capabilities/permissions、自绘标题栏、窗口尺寸持久化、TS 类型约定、`tauri dev` 与打包产物的行为差异
- 来源仅为 v2.tauri.app 官方文档、docs.rs 上的 crate 文档、tauri-apps/tauri 与 specta-rs/tauri-specta 仓库（含 issue）。二手博客一律未用；官方没写清的地方标「未证实」并说明查到哪一步

## 零、本仓库实际版本（以下结论的适用基线）

取自本仓库锁文件，不是「最新版」：

| 组件 | 版本 | 出处 |
|---|---|---|
| `tauri` | 2.12.5 | `Cargo.lock:3626-3627` |
| `tauri-build` | 2.7.4 | `Cargo.lock:3677-3678` |
| `tauri-plugin-opener` | 2.7.2 | `Cargo.lock:3757-3758` |
| `wry` / `tao` | 0.57.1 / 0.37.2 | `Cargo.lock:4888-4889`、`Cargo.lock:3569-3570` |
| `@tauri-apps/api` | 2.12.4 | `pnpm-lock.yaml:581` |
| `@tauri-apps/cli` | 2.12.4 | `pnpm-lock.yaml:655` |
| `@tauri-apps/plugin-opener` | 2.7.2 | `pnpm-lock.yaml:660` |
| `tauri-plugin-window-state` | 2.5.0 | `Cargo.lock:3850-3852` |

Rust 侧 `tauri` 只写了 `features = []`（`src-tauri/Cargo.toml:22`），即 `tray-icon`、`devtools`、`protocol-asset` 全未启用。

## 一、`#[tauri::command]` → 前端 `invoke`

**事实**

- command 必须注册进 `invoke_handler(tauri::generate_handler![...])` 才能被调用；`generate_handler!` 只能调用一次，多次调用只有最后一次生效。调用名默认是函数名，可用 `#[tauri::command(rename = "greetUser")]` 改。
  原始来源：https://v2.tauri.app/develop/calling-rust/#creating-multiple-commands  
  原始来源：https://docs.rs/tauri/2.12.5/tauri/attr.command.html
- 返回值 `T: serde::Serialize` → promise resolve；返回 `Result<T, E>` 时 `Ok` resolve、`Err` reject，**`E` 也必须实现 `serde::Serialize`**，`Err` 序列化后的值就是 reject 的值（官方示例前端 `.catch((e: ErrorKind) => {})` 直接拿到 `{ kind, message }` 对象）。
  原始来源：https://v2.tauri.app/develop/calling-rust/#error-handling
- 参数类型需 `serde::Deserialize`；参数名与前端 payload 的键默认按 **camelCase** 对齐（`invoke('my_custom_command', { invokeMessage: 'Hello!' })`），可用 `#[tauri::command(rename_all = "snake_case")]` 改成 snake_case。
  原始来源：https://v2.tauri.app/develop/calling-rust/#passing-arguments  
  原始来源：https://docs.rs/tauri/2.12.5/tauri/attr.command.html（Options → `rename_all`）
- `async fn` 命令在异步运行时执行（Tauri 用 tokio，`async_runtime` 模块文档也写明「自定义命令推荐 plain `async fn`」）；`#[tauri::command(async)]` 只对同步函数有意义。async 命令带借用参数（`&str`、`State<'_, T>`）时必须改成拥有类型，或把返回类型包成 `Result<_, _>`。
  原始来源：https://v2.tauri.app/develop/calling-rust/#async-commands  
  原始来源：https://docs.rs/tauri/2.12.5/tauri/async_runtime/index.html
- 自定义错误类型的官方做法是手写 `impl serde::Serialize for Error`（或先转成一个可序列化的枚举），示例产出 `{ kind, message }`。这正是本项目要的 `{code, message}` 形状。
  原始来源：https://v2.tauri.app/develop/calling-rust/#error-handling

**最小可用片段（Rust）**

```rust
use fsh_core::ErrorCode;

/// 桥接层错误：code 走 fsh_core 的稳定码契约，message 只供人读
#[derive(Debug, serde::Serialize)]
pub struct GuiError {
    pub code: &'static str,
    pub message: String,
}

/// 用函数而不是 `impl From` 做转换：blanket From 会与 `From<T> for T` 冲突
fn err(e: &(impl ErrorCode + std::fmt::Display)) -> GuiError {
    GuiError { code: e.error_code(), message: e.to_string() }
}

#[tauri::command]
async fn list_skills() -> Result<Vec<SkillView>, GuiError> {
    // git2 是同步阻塞调用：在 async 命令里直接跑会占住 tokio worker
    // 下面这行是形状示意，fsh_core 没有这个函数名
    tauri::async_runtime::spawn_blocking(|| fsh_core::skill::list(&hub))
        .await
        .map_err(|_| GuiError { code: "join_failed", message: "任务被取消".into() })?
        .map_err(|e| err(&e))
}
```

**最小可用片段（TS）**

```ts
export type GuiError = { code: string; message: string };

try {
  const skills = await invoke<SkillView[]>("list_skills");
} catch (e) {
  // reject 值不是 Error 实例，instanceof Error 为 false
  const { code } = e as GuiError;
}
```

**建议**

- reject 值是纯数据对象，不是 `Error`；strict 模式下 `catch` 变量是 `unknown`，必须在 IPC 封装层做一次形状守卫，别在页面里散着断言。
- `spawn_blocking` 是 `tauri::async_runtime` 导出的函数（模块文档 Functions 列表可见），git2 那条链路必须走它，否则一个慢 git 操作会卡住 tokio worker。
  原始来源：https://docs.rs/tauri/2.12.5/tauri/async_runtime/index.html

## 二、capabilities / permissions

**事实**

- **自定义 command 不需要 capability**：官方原文「By default, all commands that you registered in your app (using `tauri::Builder::invoke_handler`) are allowed to be used by all the windows and webviews of the app」。要收紧只能用 `tauri_build::AppManifest::commands(&[...])`。
  原始来源：https://v2.tauri.app/security/capabilities/
- 插件命令默认全部被 block，必须在 capabilities 里列出来（「By default all potentially dangerous plugin commands and scopes are blocked」）。
  原始来源：https://v2.tauri.app/plugin/opener/#permissions  
  原始来源：https://v2.tauri.app/plugin/window-state/#permissions
- `core:default` = `core:app:default` + `core:event:default` + `core:image:default` + `core:menu:default` + `core:path:default` + `core:resources:default` + `core:tray:default` + `core:webview:default` + `core:window:default`。
  原始来源：https://v2.tauri.app/reference/acl/core-permissions/
- 窗口操作要单独授权。官方自绘标题栏示例在 `core:window:default` 之上单列了 `core:window:allow-close`、`core:window:allow-minimize`、`core:window:allow-toggle-maximize`、`core:window:allow-start-dragging`；官方对该示例的权限表注明 `core:window:default` 包含 `core:window:allow-internal-toggle-maximize`（拖拽区内部双击用），这四条不在其中。
  原始来源：https://v2.tauri.app/learn/window-customization/#permissions
- `opener:default` = `allow-open-url` + `allow-reveal-item-in-dir` + `allow-default-urls`（只放行 `mailto:`/`tel:`/`http(s)://`）。`openPath()` 不在 default 里，必须写 `{"identifier": "opener:allow-open-path", "allow": [{"path": "..."}]}`，scope 支持 `$APPDATA` 等变量与 glob。
  原始来源：https://v2.tauri.app/plugin/opener/#permissions
- `dialog:default` = `allow-message` + `allow-save` + `allow-open`（`ask`/`confirm` 已标 DEPRECATED，是 `message` 的别名）。
  原始来源：https://v2.tauri.app/plugin/dialog/
- `window-state:default` = `allow-filename` + `allow-restore-state` + `allow-save-window-state`（仅 JS 侧调用才需要）。
  原始来源：https://v2.tauri.app/plugin/window-state/#permissions
- `src-tauri/capabilities/*.json` 默认全部启用；一旦在 `app.security.capabilities` 里显式列出，就只有列出的生效。
  原始来源：https://v2.tauri.app/security/capabilities/  
  原始来源：https://docs.rs/tauri-utils/2.10.2/tauri_utils/config/struct.SecurityConfig.html（`capabilities` 字段）

**最小 `src-tauri/capabilities/default.json`（一期自绘标题栏 + opener）**

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "主窗口默认能力",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default",
    "core:window:allow-close",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-start-dragging"
  ]
}
```

`"windows": ["main"]` 成立的前提是窗口 label 默认就是 `main`（`AppConfig.windows` 文档：「If not specified, the window’s label (its identifier) defaults to “main”」）。
原始来源：https://docs.rs/tauri-utils/2.10.2/tauri_utils/config/struct.AppConfig.html

**建议**

- 本仓库 `src-tauri/capabilities/default.json:6-9` 现在只有 `core:default` + `opener:default`：一期加标题栏时必须补那四条 window 权限，否则按钮点了前端只会 reject（ACL 拒绝），不是「没绑事件」那种错，排查时别找错方向。
- 不要为了省事加 `core:window:allow-set-title` 之类的全量权限；一张能力表只列真正调用的命令。

## 三、自绘标题栏（`decorations: false`）

**事实**

- `"decorations": false` 关掉原生边框与标题栏（WindowConfig 字段：`decorations: bool`，「Whether the window should have borders and bars」）。
  原始来源：https://docs.rs/tauri-utils/2.10.2/tauri_utils/config/struct.WindowConfig.html
- 拖拽区：在元素上加 `data-tauri-drag-region` 即可；官方示例把它放在 titlebar 的一个空 div 上，并提醒「you may need to move the rest of your content down so that the titlebar doesn’t cover it」。
  原始来源：https://v2.tauri.app/learn/window-customization/#html
- 双击最大化不是原生窗口行为，要自己接：官方「Manual Implementation」用 `mousedown` + `e.buttons === 1`，`e.detail === 2` 时 `appWindow.toggleMaximize()`，否则 `appWindow.startDragging()`。
  原始来源：https://v2.tauri.app/learn/window-customization/#manual-implementation-of-data-tauri-drag-region
- 窗口控制按钮就是 JS API：`getCurrentWindow().minimize()` / `.toggleMaximize()` / `.close()`。
  原始来源：https://v2.tauri.app/learn/window-customization/#javascript
- 高 DPI：`width`/`height`/`x`/`y`/`minWidth`/`minHeight` 全部是 **logical pixels**（字段文档原文「in logical pixels」），不需要按 `devicePixelRatio` 自己换算；WebView 里 1 CSS px 与 1 logical px 对齐。
  原始来源：https://docs.rs/tauri-utils/2.10.2/tauri_utils/config/struct.WindowConfig.html
- 未证实：内置拖拽区脚本在双击时到底调什么，官方文档没有给出脚本细节。能查到的只有两件事——`core:window:allow-internal-toggle-maximize` 属于 `core:window:default`（window-customization 权限表），且它是个真实命令。据此只能推断「内置拖拽区双击会走该命令」，不能当成承诺。

**已知问题（仓库 issue，不是文档承诺；做一期时按现状预期）**

- Windows：`decorations: false` 下窗口边缘 resize 命中区与光标不一致、拖拽与 resize 抢事件 —— https://github.com/tauri-apps/tauri/issues/7388（closed）
- Windows：`decorations(false)` 只能从上边缘 resize —— https://github.com/tauri-apps/tauri/issues/9268（closed，not planned）
- Windows：`decorations(false)` 时 WebView 内容没铺满，右侧留一条边 —— https://github.com/tauri-apps/tauri/issues/11682（open，带 unstable flag）
- macOS 与 Windows 的圆角/边框表现不一致 —— https://github.com/tauri-apps/tauri/issues/12042（open）
- `transparent: true` + `shadow: true` 时出现黑边 —— https://github.com/tauri-apps/tauri/issues/13176（closed，question）

**建议**

- 一期先不要 `decorations: false`：四页都是数据页，自绘标题栏只换来一套 Windows 边缘 resize 的实测成本，换不到功能。真要做，先做一版 Windows 真机清单：边缘 resize、最大化贴边/圆角、双击最大化、多显示器拖动，再决定是否保留。
- 若保留，双击最大化按官方「Manual Implementation」显式写 `mousedown` 逻辑，不依赖内置拖拽区的双击行为（见上面的未证实项）。

## 四、窗口尺寸/位置持久化与最小尺寸

**事实**

- `minWidth` / `minHeight` / `maxWidth` / `maxHeight` 是 `app.windows[]` 里的 WindowConfig 字段（`min_width: Option<f64>`，logical px）；另有 `preventOverflow`（防止窗口溢出工作区）。
  原始来源：https://docs.rs/tauri-utils/2.10.2/tauri_utils/config/struct.WindowConfig.html
- 官方插件 `tauri-plugin-window-state`：Rust 侧在 `setup` 里 `app.handle().plugin(tauri_plugin_window_state::Builder::default().build())`（示例带 `#[cfg(desktop)]`），JS 侧装 `@tauri-apps/plugin-window-state`；默认「all windows will remember their state when the app is being closed and will restore to their previous state on the next launch」；也可手动 `saveWindowState(StateFlags.ALL)` / `restoreStateCurrent(...)`，Rust 侧走 `AppHandleExt::save_window_state` / `WindowExt::restore_state`。插件要求 Rust ≥ 1.90。
  原始来源：https://v2.tauri.app/plugin/window-state/
- 未证实：进程被强杀（任务管理器结束进程、崩溃）时插件是否落盘——文档只写「when the app is being closed」，没有异常退出路径的说明；多显示器拔插后坐标越界的纠正策略也没有文档。
- 未证实：`minWidth`/`minHeight` 与插件恢复值的优先级（恢复一个小于 min 的旧尺寸会怎样）没有文档描述，需要实测。

**最小可用片段**

```jsonc
// tauri.conf.json → app.windows[0]
{
  "title": "Fuyao Skills Hub",
  "width": 1100,
  "height": 720,
  "minWidth": 900,
  "minHeight": 600,
  "decorations": true
}
```

```rust
// src-tauri/src/lib.rs：setup 里挂插件，桌面端才有
.setup(|app| {
    #[cfg(desktop)]
    app.handle().plugin(tauri_plugin_window_state::Builder::default().build())?;
    Ok(())
})
```

**建议**

- 用官方插件，不手写：手写要自己处理「保存时机、越界修正、多显示器、最大化还原尺寸」四件事，插件已经覆盖前三件的常见路径，省下的代码量不值当。
- 若进程强杀丢状态不可接受（一期不涉及），再考虑在窗口 `on_window_event` 的 `CloseRequested` 之外加一个定时保存——这属于二期，别提前做。

### 实施结论：先恢复再显示（2026-11-05）

本项目实际接线时出现过窗口启动闪变：窗口先按 `tauri.conf.json` 的默认尺寸/居中位置露脸，随后跳到上次关闭时的尺寸/位置/最大化状态。原因是窗口按配置**可见创建**，插件在 window ready 时才恢复——默认态已经画出来了。

固定做法：

1. `tauri.conf.json` 的窗口加 `"visible": false`，窗口先隐藏创建。
2. 插件显式带 `StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::VISIBLE`；`VISIBLE` 不能省——隐藏建窗后由插件负责 `show()`，少了它窗口不会出现。
3. 插件在 window ready 时先应用尺寸/位置/最大化，再按 `VISIBLE` 调 `show()` + `set_focus()`（`tauri-plugin-window-state` 2.5.0 的 `restore_state`）；第一次露脸即最终态。
4. `center: true` 与这个顺序不冲突：center 在窗口创建前算进初始位置（此时隐藏），插件随后覆盖为存档位置；它只决定首次无存档时居中。

本仓库在 `Builder` 上以 `.plugin(...)` 注册（`src-tauri/src/lib.rs:24-33`），不走 `setup` 里的 `app.handle().plugin(...)`；两种注册方式对恢复顺序没有影响。

验证：把窗口拉到某位置/最大化后关闭，再启动，首帧应直接是关闭时的状态；首次无存档仍按配置尺寸/居中显示。

## 五、TS strict 下的 `invoke<T>` 与类型对齐

**事实**

- `invoke` 返回 `Promise<T>`，`T` 完全由调用方给，运行时不校验；`InvokeArgs` 是 `Record<string, unknown> | number[] | ArrayBuffer | Uint8Array`。
  原始来源：https://v2.tauri.app/reference/javascript/api/namespacecore/#type-aliases（`InvokeArgs`）  
  原始来源：https://v2.tauri.app/reference/javascript/api/namespacecore/
- 参数键名由 command 侧决定：默认 camelCase，`rename_all = "snake_case"` 时用 snake_case（见 §1 出处）。
- 返回结构体字段默认按 Rust 字段名**原样**序列化。官方 Complete Example 里 Rust 字段叫 `other_val`，前端读的就是 `res.other_val`——不写 `#[serde(rename_all = "camelCase")]` 就是 snake_case。
  原始来源：https://v2.tauri.app/develop/calling-rust/#complete-example
- 官方**没有**内建 TS 类型生成。`tauri` 2.12.5 带一个可选 `specta` feature（可选依赖 `specta ^2.0.0-rc.16`），生态方案是 tauri-specta v2（README 当前指向 `2.0.0-rc.21`，明确 v2 配 Tauri v2，并支持生成事件类型）。
  原始来源：https://docs.rs/crate/tauri/2.12.5/features（`specta` feature）  
  原始来源：https://github.com/specta-rs/tauri-specta
- tauri-specta 的代价（据其 README 与文档入口）：command 要加 `#[specta::specta]`、类型要 derive `specta::Type`、在 debug 构建里导出 bindings 文件；当前是 rc 版本，API 仍可能变。
  原始来源：https://docs.rs/tauri-specta/%5E2.0.0-rc.21/tauri_specta/index.html

**最小可用片段（`src/lib/ipc.ts`，一期建议的形态）**

```ts
import { invoke } from "@tauri-apps/api/core";

export type GuiError = { code: string; message: string };
export type SkillView = { key: string; version: string; tags: string[] };

export function listSkills(): Promise<SkillView[]> {
  return invoke<SkillView[]>("list_skills");
}
```

**建议**

- Rust 侧所有返回结构体统一 `#[serde(rename_all = "camelCase")]`，前端就按 camelCase 写类型；错误对象字段本来就叫 `code`/`message`，不受影响。
- 一期命令数在十来个量级：手写 `ipc.ts` 一张表足够，别引 rc 依赖。等命令数过 20，或真出现「前端字段名漂移」事故，再上 tauri-specta（那时它的收益才是可验证的）。
- `invoke` 的 `T` 是断言不是校验：`as` 不产生运行时代价，也不会替你发现 Rust 改了字段。跨端契约的唯一保险是「Rust 侧字段改名必须同时改 `ipc.ts`」，把这句话写进规范比引类型生成器便宜。

## 六、`pnpm tauri dev` 与打包产物的差异

**事实**

- dev：页面来自 `build.devUrl`（本仓库 `http://localhost:1420`，`src-tauri/tauri.conf.json:8`）+ `beforeDevCommand: pnpm dev`，走 Vite dev server，有 HMR。
  原始来源：https://v2.tauri.app/reference/config/#buildconfig
- build：`beforeBuildCommand: pnpm build` 产出 `../dist`，`generate_context!` 把 `frontendDist` 的资源**编进二进制**（原文：「The context embeds the frontend assets, the application icons, the resolved Access Control List and the parsed configuration into the binary」），页面从自定义协议加载。`WebviewUrl::App` 的文档例子直接写的就是 `tauri://localhost/users/john`。
  原始来源：https://docs.rs/tauri/2.12.5/tauri/macro.generate_context.html  
  原始来源：https://docs.rs/tauri/2.12.5/tauri/enum.WebviewUrl.html
- 因此 dev 的 origin 是 `http://localhost:1420`，打包后是 `tauri://localhost`（Linux/macOS）。Windows 侧是 `http://tauri.localhost` 这一族：CSP 文档示例里 IPC 与 asset 协议都写成 `http://ipc.localhost` / `http://asset.localhost`，且 WindowConfig 有 `useHttpsScheme` 可切 https。
  原始来源：https://v2.tauri.app/security/csp/  
  原始来源：https://docs.rs/tauri-utils/2.10.2/tauri_utils/config/struct.WindowConfig.html（`use_https_scheme`）
- **未证实**：Windows 上 app 协议串的官方原文（`http://tauri.localhost`）没有单独一段写明；只查到同族协议的写法。要确认就在 Windows 上 `console.log(location.origin)` 打一次，比读文档快。
- 后果：`localStorage` / `sessionStorage` / `IndexedDB` 按 origin 隔离 → dev 里存的状态打包版读不到，反之亦然。写死 `http://localhost:1420` 的绝对 URL、依赖 dev server 代理/中间件的逻辑，打包后全部失效。
- CSP：`csp` 注入打包产物；`devCsp` 未设时 `csp` 也会注入 dev。Tauri 在编译期扫描前端资源并追加 nonce/hash，而 dev 页面不是这些资源——所以设了严格 CSP 时 dev 更容易炸，需要 `devCsp` 单独放宽。本仓库 `"csp": null`（`src-tauri/tauri.conf.json:21`）= 完全不注入。
  原始来源：https://docs.rs/tauri-utils/2.10.2/tauri_utils/config/struct.SecurityConfig.html（`csp` / `dev_csp` 字段）  
  原始来源：https://v2.tauri.app/security/csp/
- `window.__TAURI__`：默认不注入，`withGlobalTauri` 默认 `false`（配置参考的默认值对象里就是 `"withGlobalTauri": false`），dev 与 build 一致；只有显式开启后 `@tauri-apps/api` 才同时挂在 `window.__TAURI__.core` 上。
  原始来源：https://v2.tauri.app/reference/config/#appconfig  
  原始来源：https://v2.tauri.app/reference/javascript/api/namespacecore/
- 资源路径：`frontendDist` 内是相对路径，自定义协议下以 `tauri://localhost/` 为根，Vite 默认 `base: "/"` 可用；但引用 `src-tauri` 之外的本地文件（如 hub 里的图片）不能用 `file://`，要走 `asset:` 协议——它默认关闭（配置参考默认值 `"assetProtocol": { "enable": false, "scope": [] }`），需要 `app.security.assetProtocol.enable` + scope，Rust 侧还要 `protocol-asset` feature，CSP 里放行 `asset:` / `http://asset.localhost`。
  原始来源：https://v2.tauri.app/reference/config/#appconfig  
  原始来源：https://v2.tauri.app/security/csp/  
  原始来源：https://docs.rs/crate/tauri/2.12.5/features（`protocol-asset`）
- 热更新：只有 dev 有 HMR；打包版改前端必须重新 `pnpm tauri build`（资源在二进制里，不是外部目录）。

**最小可用片段（把差异挡在一处）**

```ts
// 状态一律落 Rust 侧（hub 的 data/），webview 存储只放可丢弃的 UI 偏好
export const isTauri = () => "__TAURI_INTERNALS__" in window;
```

**建议**

- 一切需要跨 dev/build 存活的状态（窗口状态、最近打开、筛选偏好里要紧的部分）交给 Rust；`localStorage` 只放「丢了也无所谓」的东西。这条能一次性消掉 origin 差异带来的一整类 bug。
- 一期把 `csp` 从 `null` 改成具体策略的时机，是引入 `asset:` 或远程字体的时候；在那之前 `null` 只是「没有 XSS 兜底」，不影响功能。

## 七、与本仓库现状的对照（冲突与落点）

- `src-tauri/capabilities/default.json:6-9` 只有 `core:default` + `opener:default` → 自绘标题栏前必须补 §2 的四条 window 权限，否则按钮静默失败。
- `src-tauri/tauri.conf.json:13-19` 没有 `minWidth`/`minHeight`，没有 `decorations`（默认 `true`），没有 `withGlobalTauri`（默认 `false`，与「只用 npm 包」的现状一致）。
- `src-tauri/tauri.conf.json:21` `"csp": null` → 当前无 CSP；一旦要 `asset:` 或外部字体，先定 CSP 再动前端。
- `src-tauri/Cargo.toml:22` `tauri = { version = "2", features = [] }` → 没有 `tray-icon`/`devtools`/`protocol-asset`；用 `asset:` 必须加 `protocol-asset`。
- `src-tauri/src/lib.rs:6-9` 还没有 `invoke_handler`，一条命令都没注册；`manage(log_guard)` 已占用状态管理，加命令时注意别把 `WorkerGuard` 抢走。
- 版本对齐没问题：Rust `tauri` 2.12.5 / JS `@tauri-apps/api` 2.12.4 同一 minor；`tauri-plugin-opener` 与 JS 插件都是 2.7.2。
- 与设计文档不冲突的地方：`docs/设计文档.md` §2.1 说 `src-tauri` 是「薄桥，command 只调 fsh-core」——Tauri 的 command 机制正好是这个形状，无需额外抽象层。

## 八、四句核心结论

1. 自定义 command 不需要 capability，插件命令必须；一期做标题栏时那四条 `core:window:allow-*` 是硬门槛，漏了就是前端 reject。
2. `{code, message}` 不用发明协议：`Result<T, E>` 里 `E` 的 serde 形状就是 reject 值，官方示例就是自定义结构体 + 手写 `Serialize`；前端拿到的不是 `Error` 实例。
3. `decorations: false` 的代价集中在 Windows 边缘 resize 与圆角/贴边（四个未关的 issue），一期四页数据界面不值得付这个成本。
4. dev 与 build 的差异集中在 origin（webview 存储不通用）、CSP 注入方式与资源内嵌；官方没有 TS 类型生成，命令数少时手写 `ipc.ts` 比引 rc 版 tauri-specta 划算。
