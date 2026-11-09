import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  FolderOpen,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Search,
  Sun,
} from "lucide-react";
import { lazy, Suspense, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { FOCUS_RING } from "@/lib/focus-ring";
import { hubQueries } from "@/lib/hub-queries";
import { api } from "@/lib/ipc";
import { applyTheme, readTheme, type Theme } from "@/lib/theme";
import { useHubMutation } from "@/lib/use-hub-mutation";
import type { FocusTarget } from "@/lib/use-jump-scroll";
import { cn } from "@/lib/utils";
import { Centered, ErrorBlock } from "./feedback";
import { InitGuide } from "./init-guide";
import { PAGES, type PageKey } from "./pages";

// 命令面板按需装载：cmdk 与 Base UI 弹层那一套不进首屏，第一次打开才取回来
const CommandPalette = lazy(() =>
  import("./command-palette").then((module) => ({
    default: module.CommandPalette,
  })),
);

/** 应用壳：侧栏 + 顶栏 + 页面容器；页面本身由注册表提供 */
export function Shell() {
  const [page, setPage] = useState<PageKey>("library");
  const [collapsed, setCollapsed] = useState(false);
  const [theme, setTheme] = useState<Theme>(() => readTheme());
  const [paletteOpen, setPaletteOpen] = useState(false);
  // 面板打开过一次就保持挂载：卸载会让 base-ui 的关闭动画来不及跑
  const [paletteMounted, setPaletteMounted] = useState(false);
  // 命令面板跳技能或 agent：记目标页，seq 让同一个键第二次跳也能再次触发
  const [focus, setFocus] = useState<({ page: PageKey } & FocusTarget) | null>(
    null,
  );
  const queryClient = useQueryClient();

  const capabilities = useQuery(hubQueries.capabilities);
  // 初始化走共享写出口：成功后全量失效，环境查询跟着重拉；
  // 它不参与写操作串行——首次初始化前没有别的写操作在跑
  const init = useHubMutation({ mutationFn: api.initHub, scope: null });
  // 打开数据目录同样走写出口：失败文案只在这里写一次；它不改读侧数据，
  // 因此不占写串行位、成功后也不失效查询
  const openDir = useHubMutation({
    mutationFn: api.openHubDir,
    failure: "无法打开数据目录",
    scope: null,
    invalidate: false,
  });

  // Ctrl+K 开合命令面板，Esc 关闭
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
      if (event.key === "Escape") setPaletteOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // 第一次打开面板时才挂载它，这一帧才去取面板代码
  useEffect(() => {
    if (paletteOpen) setPaletteMounted(true);
  }, [paletteOpen]);

  const toggleTheme = () => {
    const next: Theme = theme === "dark" ? "light" : "dark";
    applyTheme(next);
    setTheme(next);
  };

  const current = PAGES.find((entry) => entry.key === page) ?? PAGES[0];
  const Page = current.component;

  return (
    <div className="flex h-screen overflow-hidden bg-background text-foreground">
      <aside
        className={cn(
          "flex shrink-0 flex-col border-r border-sidebar-border bg-sidebar transition-[width] duration-100",
          collapsed ? "w-16" : "w-58",
        )}
      >
        <div
          className={cn(
            "flex h-12 shrink-0 items-center border-b border-sidebar-border",
            collapsed ? "justify-center" : "px-3",
          )}
        >
          <span className="text-sm font-semibold tracking-tight">Fuyao</span>
          {collapsed ? null : (
            <span className="ml-1.5 text-xs text-muted-foreground">
              Skills Hub
            </span>
          )}
        </div>

        <nav aria-label="主导航" className="flex-1 overflow-y-auto p-2">
          {PAGES.map(({ key, label, icon: Icon }) => (
            <button
              key={key}
              type="button"
              onClick={() => setPage(key)}
              aria-current={page === key ? "page" : undefined}
              className={cn(
                "flex h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-sm transition-colors duration-100 hover:bg-sidebar-accent",
                page === key
                  ? "bg-sidebar-accent font-medium"
                  : "text-muted-foreground",
                collapsed && "justify-center px-0",
              )}
            >
              <Icon className="size-4" strokeWidth={1.75} aria-hidden="true" />
              {collapsed ? null : <span>{label}</span>}
            </button>
          ))}
        </nav>

        <div
          className={cn(
            "shrink-0 border-t border-sidebar-border p-2",
            collapsed && "flex justify-center",
          )}
        >
          <Button
            variant="ghost"
            size={collapsed ? "icon-sm" : "sm"}
            onClick={() => setCollapsed((value) => !value)}
            aria-label={collapsed ? "展开侧栏" : "折叠侧栏"}
            className={cn(
              !collapsed && "w-full justify-start text-muted-foreground",
            )}
          >
            {collapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
            {collapsed ? null : <span>折叠侧栏</span>}
          </Button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-border px-4">
          <div className="flex min-w-0 items-baseline gap-2">
            <h1 className="text-sm font-semibold">{current.label}</h1>
            <span className="truncate text-xs text-muted-foreground">
              {current.description}
            </span>
          </div>
          <div className="flex items-center gap-1">
            {/* 搜索入口：外形是搜索框，点击开命令面板 */}
            <button
              type="button"
              onClick={() => setPaletteOpen(true)}
              aria-label="搜索页面或技能"
              className={cn(
                "flex h-8 w-56 items-center gap-2 rounded-lg border border-input bg-background px-2.5 text-xs text-muted-foreground transition-colors duration-100 hover:bg-accent/50",
                FOCUS_RING,
              )}
            >
              <Search className="size-3.5 shrink-0" aria-hidden="true" />
              <span className="truncate">搜索页面或技能…</span>
            </button>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => openDir.mutate()}
              aria-label="打开数据目录"
            >
              <FolderOpen />
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={toggleTheme}
              aria-label={theme === "dark" ? "切换到亮色" : "切换到暗色"}
            >
              {theme === "dark" ? <Sun /> : <Moon />}
            </Button>
          </div>
        </header>

        <main className="min-w-0 flex-1 overflow-auto">
          {capabilities.isPending ? <Centered>正在读取环境…</Centered> : null}
          {capabilities.isError ? (
            <ErrorBlock
              error={capabilities.error}
              onRetry={() => capabilities.refetch()}
            />
          ) : null}
          {capabilities.data && !capabilities.data.initialized ? (
            <InitGuide
              hubRoot={capabilities.data.hub_root}
              pending={init.isPending}
              error={init.error}
              onInit={() => init.mutate()}
              onOpenDir={() => openDir.mutate()}
            />
          ) : null}
          {capabilities.data?.initialized ? (
            <Page focusKey={focus?.page === page ? focus : null} />
          ) : null}
        </main>
      </div>

      {paletteMounted ? (
        <Suspense fallback={null}>
          <CommandPalette
            open={paletteOpen}
            onOpenChange={setPaletteOpen}
            onNavigate={setPage}
            onOpenSkill={(key) => {
              setFocus((prev) => ({
                page: "library",
                key,
                seq: (prev?.seq ?? 0) + 1,
              }));
              setPage("library");
            }}
            onOpenAgent={(key) => {
              setFocus((prev) => ({
                page: "agents",
                key,
                seq: (prev?.seq ?? 0) + 1,
              }));
              setPage("agents");
            }}
            onOpenDir={() => openDir.mutate()}
            onRefresh={() => {
              void queryClient.invalidateQueries();
            }}
          />
        </Suspense>
      ) : null}
    </div>
  );
}
