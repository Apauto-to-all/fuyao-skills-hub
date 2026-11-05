# GUI 一期：shadcn（base-nova）+ Tailwind v4 的组件与主题边界

- 调研日期：2026-11-05
- 对应：GUI 一期规格（组件清单、主题定制边界、图表选型）
- 范围：`components.json` 里 `style: base-nova` 的真实语义、`@theme inline` 与 oklch 变量的分层、`cn` 单一来源、桌面端（Tauri/WebView2）图表选型、React 19 + Tailwind v4 已知坑。
- 来源：ui.shadcn.com 文档与 registry JSON、tailwindcss.com v4 文档、base-ui.com 文档、react.dev 文档、本仓 `node_modules` 内已安装包的源码与类型；体积数字为本地对 npm 官方 dist 文件实测 gzip。下文“事实”与“建议”分开，查不到的标“未证实”。

## 一、`base-nova` 是什么

**事实**

- `style` 在 v4 里是“primitive 库 + 风格族”的复合键：`base-nova` = `base`（Base UI）+ `nova`。CLI 用 `parsePresetStyle()` 拆开，`base` 决定用哪套 primitive，`style` 决定从 registry 取哪套文件。实测 `parsePresetStyle("base-nova")` → `{"base":"base","style":"nova"}`；`PRESET_BASES` = `radix | base | aria`，`PRESET_STYLES` = `nova | vega | maia | lyra | mira | luma | sera | rhea`。
  出处：https://ui.shadcn.com/docs/cli （`-b, --base <base>` 说明为 “the component library to use. (base, radix, aria)”）；本仓 `node_modules/shadcn@4.23.0`、`node_modules/.pnpm/@shadcn+registry@0.2.1/.../dist/preset/preset.d.ts`
- 它既不是独立 registry，也不是 preset code。组件文件取自 `https://ui.shadcn.com/r/styles/base-nova/<item>.json`，`components.json` 的 `style` 原样拼进这条路径。本仓 `pnpm exec shadcn preset resolve` 输出的 preset code 是 `b2fA`（style=nova、baseColor=neutral、theme=neutral、chartColor=neutral、iconLibrary=lucide、font=geist、radius=default），**不含 base** —— base 只体现在 `style` 字段里。
  出处：https://ui.shadcn.com/docs/cli （`preset resolve` 一节）
- `pnpm shadcn add <name>` 的 name 就是 `<item>`。2026-11-05 逐项拉 registry JSON 全部 HTTP 200：`sidebar`、`command`、`table`、`sheet`、`drawer`、`tabs`、`tooltip`、`sonner`、`dialog`、`card`、`badge`、`button`、`select`、`combobox`、`field`、`empty`、`spinner`、`kbd`、`item`、`input-group`、`native-select`、`button-group`、`alert`、`avatar`、`accordion`、`chart`、`toast` 都存在。
- 依赖关系决定“add 一次会带进多少东西”（registry JSON 的 `dependencies` / `registryDependencies`）：
  - `sidebar` → 依赖项 `button, input, separator, sheet, skeleton, tooltip, use-mobile`（共 7 项；`use-mobile` 是 `registry:hook`，会落到 `@/hooks/use-mobile.ts`，本仓目前没有 `src/hooks/`）。
  - `command` → 依赖 `cmdk`（不是 Base UI）。
  - `sheet` → 用 `@base-ui/react/dialog` 做的侧滑面板；`drawer` → 用 `@base-ui/react/drawer`。两者都在，不是二选一。
  - `chart` → 依赖 `recharts@3.8.0`（钉版本）。
  - `toast` → `@base-ui/react/toast` + `ToastPrimitive.createToastManager()`，无 next-themes。
  - `sonner` → 依赖 `sonner` + `next-themes`，源码 `import { useTheme } from "next-themes"`。
- `data-table` 不是 registry 项（`styles/base-nova/data-table.json` 返回 404）。它是教程：先 `pnpm shadcn add table`，再自行安装 `@tanstack/react-table`（文档用 v9，feature 式 API）。
  出处：https://ui.shadcn.com/docs/components/data-table

**建议**

- 一期只 add 真正要用的项：`sidebar` 会顺带拉 7 个依赖项和一个 hook。
- 通知用 `toast`，不用 `sonner`：CLI 常量把 sonner 在 base 项目里从选择列表隐藏（`COMPONENTS_HIDDEN_FROM_SELECTION: [{ name: "sonner", hiddenIn: ["base"] }]`），且 sonner 项硬依赖 next-themes。注意这与 shadcn 文档“toast 已弃用、改用 sonner”的说法相反，见第六节。
- `data-table` 走 `table` + TanStack Table v9，不要指望 `add data-table`。

## 二、主题 token 与 teal 强调色

**事实**

- 分层：`:root` / `.dark` 只存裸值（本仓全是 `oklch(...)`，含 `oklch(1 0 0 / 10%)` 这类带 alpha 的），`@theme inline` 把 `--color-*` 映射到这些变量。`inline` 不是可选优化：不加 inline 时 `--color-primary: var(--primary)` 的 `var()` 在声明处（`:root`）就解析完，`.dark` 再改 `--primary` 不会传导；官方把它描述为“utility 使用变量的值，而不是引用变量本身”。
  出处：https://tailwindcss.com/docs/theme#referencing-other-variables ；本仓 `src/index.css:8-49`（`@theme inline` 段）、`src/index.css:51-118`（`:root` / `.dark`）
- `:root` 放“不映射成 utility”的变量，`@theme` 放“要变成 utility”的 token，是官方分工。
  出处：https://tailwindcss.com/docs/theme#why-theme-instead-of-root
- 加新 token 的官方配方：`:root` 与 `.dark` 各写一份值，再在 `@theme inline` 加一行 `--color-<name>: var(--<name>)`。
  出处：https://ui.shadcn.com/docs/theming#adding-new-tokens
- 语义分工（官方表述）：`primary` = 高强调动作与品牌面（default Button、选中态、badge）；`accent` = 交互 hover/focus/active 面（ghost 按钮、菜单高亮、选中行）；`ring` = 焦点环。
  出处：https://ui.shadcn.com/docs/theming#theme-tokens
- 生成组件实际读的 token（决定改哪个变量有效）：
  - `default` 按钮 = `bg-primary text-primary-foreground hover:bg-primary/80`（`src/components/ui/button.tsx:10`）
  - Tabs 选中 = `data-active:bg-background data-active:text-foreground`，不吃 `primary`
  - 侧栏选中 = `data-active:bg-sidebar-accent`，吃 `--sidebar-accent`
  出处：registry `styles/base-nova/tabs.json`、`styles/base-nova/sidebar.json`
- 不透明度修饰符可用在 CSS 变量色上：`bg-primary/80`、`ring-ring/50` 在 v4 走 `color-mix()`。
  出处：https://tailwindcss.com/docs/functions-and-directives#alpha-function
- 对比度实测（oklch → 线性 sRGB → WCAG 比值，取值来自 `node_modules/tailwindcss/theme.css` 默认色板）：
  - teal-600 `oklch(0.60 0.118 184.704)`：近白字在其上 **3.56:1**，白底作文字色 **3.72:1** —— 都不到 AA 4.5。
  - teal-700 `oklch(0.511 0.096 186.391)`：近白字 **5.21:1**，白底作文字色 **5.44:1** —— 过 AA。
  - teal-400 `oklch(0.777 0.152 181.912)`：近黑字在其上 **9.46:1**，暗底作文字色 **10.45:1**。

**建议**

- 最小改法（零新增 token、零改生成组件）：亮色 `--primary: oklch(0.511 0.096 186.391)`（teal-700）+ `--primary-foreground: oklch(0.985 0 0)`；暗色 `--primary: oklch(0.777 0.152 181.912)`（teal-400）+ `--primary-foreground: oklch(0.205 0 0)`；同步 `--ring`、`--sidebar-primary`、`--sidebar-ring`。默认按钮、焦点环、badge 立即变 teal。
- 选中态若也要 teal：Tabs 与侧栏的选中色不吃 `primary`，只有两条路 —— (a) 在 `src/index.css` 加按 `data-slot` / `data-active` 的覆盖规则；(b) 自研组件里用新增的 `--color-brand`（官方“加新 token”配方）。不要为了选中态去改 `--accent`：它同时是全局 hover 底色，会整片变青。
- 暗色切换：`.dark` 挂 `<html>`，偏好存 `localStorage`，**首屏不闪只能靠 `index.html` 里的内联脚本**（在 bundle 之前执行）。Tailwind 官方示例明确写 “best to add inline in `head` to avoid FOUC”。
  出处：https://tailwindcss.com/docs/dark-mode#with-system-theme-support
- shadcn 的 Vite 配方（ThemeProvider + `useEffect` 里加 class）是晚一帧的写法，照抄会闪一次；只借用它的 context API 形状。
  出处：https://ui.shadcn.com/docs/dark-mode/vite

## 三、`cn` 单一来源约定

**事实**

- base-nova 的所有生成组件都写 `import { cn } from "cn"`（button / dialog / tabs / sidebar / sheet / toast 均如此）。
- CLI 的 import 重写只命中“指向 `@/lib/utils`（或由 `aliases.utils` 派生的 `<prefix>/lib/utils`）且具名导入 `cn`”的声明；`from "cn"` 不匹配任何分支，原样保留。判定在 `@shadcn/registry` 的 `transform-import`：先算 `s = <aliases.utils 派生的>/lib/utils`，只有 `specifier === s || specifier === "@/lib/utils"` 时才替换成 `aliases.utils`。
- registry 的 `utils` 项内容就是 `export { cn } from "cn"`，与本仓 `src/lib/utils.ts:1` 完全一致；base-nova 的 `index` 项把它列为 `registryDependencies`。
  出处：https://ui.shadcn.com/r/styles/base-nova/utils.json
- `aliases.utils` 同时决定 `utils` 项落到哪个文件（本仓 `@/lib/utils` → `src/lib/utils.ts`）。

**建议**

- 现有约定成立，`components.json` 不用改：生成组件保持 `from "cn"`，自研组件从 `@/lib/utils` 取；两者最终是同一个实现（`cn` 0.5.2，clsx + tailwind-merge 的编译版替代）。
- 唯一会被 `aliases.utils` 改写的情形是“手写组件从 `@/lib/utils` 导入 `cn`”，而 CLI 只处理它当次写入的文件，不会回头改已有文件，现状没有风险。

## 四、图表选型（桌面端 Tauri/WebView2）

**事实**（体积为本地实测：拉 npm 官方 dist 文件后 `zlib.gzipSync(level:9)`）

| 方案 | 实测体积 | React 19 | 说明 |
|---|---|---|---|
| 纯 SVG/CSS 自绘 | 0 | — | 只有固定形状（走势线、柱、迷你图）才划算 |
| Recharts 3.8.0 | `umd/Recharts.js` 531.8KB → **139.8KB gzip**（整库 UMD） | ✅ peer `^16.8 \|\| ^17 \|\| ^18 \|\| ^19` | shadcn `chart` 的底座；依赖含 `@reduxjs/toolkit`、`immer`、`victory-vendor`、`react-redux` |
| visx 4.0.0 | 无单包体积；核心 d3 依赖 d3-shape 7.4KB / d3-scale 4.7KB / d3-array 5.8KB gzip | ✅ peer `^18 \|\| ^19` | 低层原语，轴、刻度、交互要自己写 |
| ECharts 6.1.0 | `dist/echarts.min.js` 1095.6KB → **360.4KB gzip**；`echarts.simple.min.js` → **165.7KB gzip** | 无 React 绑定（`package.json` 无 `peerDependencies`） | canvas 渲染、命令式，要自己包 React 生命周期与 resize |

出处：https://unpkg.com/recharts@3.8.0/package.json 、https://unpkg.com/echarts@6.1.0/ 、https://unpkg.com/@visx/shape@4.0.0/package.json 、https://ui.shadcn.com/docs/components/chart

**建议**

- 选 **shadcn `chart`（recharts 3.8.0）**：本仓栈的 house component，`ChartContainer` / `ChartConfig` 直接把 `--chart-*` 与 `--color-<key>` 接上主题，tooltip / legend 现成；桌面端资源从本地加载，140KB gzip 不是瓶颈。
- 只有“一条趋势线、无交互”时才用纯 SVG，别为它引库。
- 不选 ECharts：整库 360KB gzip、无官方 React 绑定、canvas 与 Tailwind token 要另写一层。不选 visx：省下的体积换来自行实现轴与交互，一期不值。
- 口径提醒：上表是“整库 dist”体积，不是 tree-shake 后的应用增量；recharts 在真实应用里的增量**未实测**（本仓不许安装依赖）。

## 五、React 19 + Tailwind v4 的坑

**事实**

- `forwardRef` 已不需要：React 19 里 ref 直接作为 prop 传，官方 API 页标 Deprecated。
  出处：https://react.dev/reference/react/forwardRef
- shadcn 生成组件已是无 forwardRef 的函数组件 + `data-slot`：`function Button({ className, variant = "default", size = "default", ...props }: ButtonPrimitive.Props & VariantProps<typeof buttonVariants>)`，根节点带 `data-slot="button"`。
  出处：`src/components/ui/button.tsx:42-55`；https://ui.shadcn.com/docs/tailwind-v4
- Base UI 与 Radix 的差异（照抄旧片段必踩）：
  - 组合用 `render` prop，不是 `asChild`。`render` 会合并 className（render 元素在前）、style（render 胜）、事件（都保留）、ref（都保留）。
    出处：https://base-ui.com/react/handbook/composition
  - 状态属性是 `data-open` / `data-closed` / `data-active` / `data-selected`，不是 Radix 的 `data-state="open"`。本仓 `shadcn/tailwind.css` 已提供这些 `@custom-variant` 垫片，所以 `data-open:animate-in` 这类类名可用。
    出处：`node_modules/shadcn/dist/tailwind.css:18-80`；registry `styles/base-nova/dialog.json`
  - 部件命名不同：Dialog 的遮罩是 `DialogPrimitive.Backdrop`（Radix 叫 `Overlay`），面板是 `DialogPrimitive.Popup`；Tabs 是 `Tabs.Tab` / `Tabs.List` / `Tabs.Panel`。
  - 受控回调多一个事件参数：`onValueChange(value, eventDetails)`，`value` 可为 `null`（无选中）；非受控用 `defaultValue`。
    出处：https://base-ui.com/react/components/tabs
  - 包名历史：`@base-ui-components/react` 已改名 `@base-ui/react`，支持 React 17+。
    出处：https://base-ui.com/react/overview/about
- 并发渲染下受控组件的硬约束：Transition 更新不能用来驱动文本输入（原文 “Transition updates can't be used to control text inputs.”）。输入框的值必须同步更新，异步结果走 `useOptimistic` / `useActionState` 或单独的 pending 态。
  出处：https://react.dev/reference/react/startTransition
- Tailwind v4 的写法变化：
  - 没有 `tailwind.config.js`（本仓 `components.json` 的 `tailwind.config` 是空串），配置全在 CSS：`@theme` / `@theme inline`、`@custom-variant`、`@utility`。
  - `@apply` 在 `<style>` 块或 CSS modules 里要先 `@reference` 主样式表，否则拿不到主题与自定义变体。
    出处：https://tailwindcss.com/docs/functions-and-directives#reference-directive
  - 自定义变体写法：`@custom-variant dark (&:where(.dark, .dark *))`。本仓写的是 `(&:is(.dark *))`，只匹配 `.dark` 的后代，不匹配 `.dark` 自身。
    出处：https://tailwindcss.com/docs/dark-mode#toggling-dark-mode-manually ；本仓 `src/index.css:6`
- `ChartContainer` 必须有高度（`min-h-*` / `aspect-*` / `h-*`），否则 `ResponsiveContainer` 首次测不到尺寸、图不显示；折叠面板里首次渲染也会踩这条。
  出处：https://ui.shadcn.com/docs/components/chart

**建议**

- 新写组件一律“函数组件 + `data-slot` + 透传 props”，不新写 `forwardRef`。
- 抄任何 shadcn 片段前先看是不是 Radix 版：`asChild` → `render`，`Overlay` → `Backdrop`。
- 主题 class 在 `index.html` 内联脚本里定，React 只负责写 `localStorage` 与同步 state。

## 六、与仓库现有假设的冲突 / 待确认

1. `src/index.css:6` 的 `@custom-variant dark (&:is(.dark *))` 只匹配 `.dark` 的后代。现在 `.dark` 在 `<html>`、样式都打在子节点上，所以能工作；一旦给 `<html>` / `<body>` 写 `dark:` 就会失效。官方写法是 `:where(.dark, .dark *)`。
2. `--chart-1..5` 在本仓是灰阶（`oklch(0.87 0 0)` 等，`src/index.css:70-74`），图表会全灰；官方默认是彩色。要上图表就得换这 5 个值。
3. 通知组件的选择与 shadcn 文档相反：文档说“toast 弃用，改用 sonner”，但 registry 常量把 sonner 在 base 项目里隐藏，且 sonner 项依赖 next-themes。一期用 `toast`。
4. `chart` 项把 `recharts` 钉在 `3.8.0`；若 `package.json` 里已装别的版本，`pnpm shadcn add chart` 会按 registry 版本再装一次。
5. `components.json` 的 `style`、`tailwind.baseColor`、`tailwind.cssVariables` 官方标注“初始化后不可改”；换 base（Base UI ↔ Radix）等于重装组件，不能靠改字段。
6. 未证实：recharts 在真实应用里 tree-shake 后的增量体积；visx 一个完整折线图的实际打包体积（本仓不许安装依赖，未实测）。

## 三句核心结论

1. `base-nova` 是 `style` 复合键（base = Base UI + 风格族 nova），组件取自 `r/styles/base-nova/<name>.json`；题目列的 sidebar / command / table / sheet / drawer / tabs / tooltip / dialog / card / badge 全部存在，`data-table` 只是教程。
2. 主题定制的最小路径是把 `--primary` / `--primary-foreground`（连同 `--ring`）覆盖成 teal；`@theme inline` 的分层保证 `.dark` 覆盖能传导。首屏不闪要靠 `index.html` 内联脚本，shadcn 的 Vite 配方会晚一帧。
3. `cn` 约定在 base-nova 下天然成立（生成文件 `from "cn"`，CLI 不重写）；图表选 recharts（shadcn chart）；React 19 下不写 `forwardRef`，也不要把 `asChild` 带进 Base UI。
