import { useQuery } from "@tanstack/react-query";
import { useLayoutEffect, useState } from "react";
import { hubQueries } from "@/lib/hub-queries";
import type { AgentUsage } from "@/lib/ipc";
import { type FocusTarget, useJumpScroll } from "@/lib/use-jump-scroll";
import { cn } from "@/lib/utils";
import { Centered, ErrorBlock } from "./feedback";
import { FOCUS_RING } from "./library/parts";

const SOURCE_LABELS: Record<string, string> = {
  built_in: "内置",
  user: "用户覆盖",
};

type Props = {
  /** 命令面板跳 agent：seq 让同一个键第二次跳也能触发定位 */
  focusKey?: FocusTarget | null;
};

/** Agent 页：表格看数量和来源，检查器从 agent 反查用它的项目与技能 */
function UsageSection({
  title,
  empty,
  usages,
}: {
  title: string;
  empty: string;
  usages: AgentUsage[];
}) {
  return (
    // aria-label 让这一段落成为可命名区域，读屏与测试都按段名定位
    <section aria-label={title}>
      <h3 className="border-b border-border px-4 py-1.5 text-xs font-medium text-muted-foreground">
        {title}
      </h3>
      {usages.length === 0 ? (
        <p className="px-4 py-3 text-xs text-muted-foreground">{empty}</p>
      ) : (
        <ul>
          {usages.map((usage) => (
            <li
              key={`${usage.project ?? "global"}:${usage.key}`}
              className="border-b border-border/60 px-4 py-2"
            >
              <p
                className="truncate text-sm"
                title={usage.project ?? "全局技能目录"}
              >
                {usage.project ?? "全局"}
              </p>
              <p className="truncate text-xs text-muted-foreground">
                {usage.key} · {usage.version ? `v${usage.version}` : "跟随最新"}
              </p>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function AgentsPage({ focusKey }: Props) {
  // 首帧就按定位目标渲染，避免先画出首行再跳
  const [selected, setSelected] = useState<string | null>(
    focusKey?.key ?? null,
  );
  const agents = useQuery(hubQueries.agents);

  // 页面已开着时再跳：把目标写进选中；同键重跳交给滚动效果的 seq 依赖
  useLayoutEffect(() => {
    if (focusKey) setSelected(focusKey.key);
  }, [focusKey]);

  const rows = agents.data ?? [];
  const active = rows.find((row) => row.key === selected) ?? rows[0];
  // 按 scope 拆成两段：全局用法没有项目路径，项目用法一定有
  const projectUsages = (active?.usages ?? []).filter(
    (usage) => usage.scope === "project",
  );
  const globalUsages = (active?.usages ?? []).filter(
    (usage) => usage.scope === "global",
  );
  // 命令面板跳转才居中；点击行保持最小滚动，已可见时不动
  const activeRow = useJumpScroll<HTMLTableRowElement>({
    activeKey: active?.key,
    seq: focusKey?.seq ?? 0,
    jumpKey: focusKey?.key,
  });

  if (agents.isPending) return <Centered>正在读取 agent…</Centered>;
  if (agents.isError)
    return <ErrorBlock error={agents.error} onRetry={() => agents.refetch()} />;

  return (
    <div className="flex h-full min-h-0">
      <div className="min-w-0 flex-1 overflow-auto">
        <table className="w-full table-fixed border-collapse text-sm">
          <colgroup>
            <col />
            <col className="w-20" />
            <col />
            <col className="w-16" />
          </colgroup>
          <thead>
            <tr className="border-b border-border text-left text-xs text-muted-foreground">
              <th scope="col" className="px-4 py-2 font-medium">
                Agent
              </th>
              <th scope="col" className="px-3 py-2 font-medium">
                来源
              </th>
              <th scope="col" className="px-3 py-2 font-medium">
                技能目录
              </th>
              <th scope="col" className="px-3 py-2 font-medium tabular-nums">
                使用数
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr
                key={row.key}
                ref={row.key === active.key ? activeRow : undefined}
                onClick={() => setSelected(row.key)}
                className={cn(
                  "cursor-pointer border-b border-border/60 transition-colors duration-100",
                  row.key === active.key ? "bg-brand/5" : "hover:bg-accent/50",
                )}
              >
                <td className="truncate px-4 py-1.5">
                  {/* 整行可点；按钮只是键盘入口，点击冒泡到行 */}
                  <button
                    type="button"
                    aria-current={row.key === active.key ? "true" : undefined}
                    className={cn("max-w-full truncate text-left", FOCUS_RING)}
                    title={row.display_name}
                  >
                    {row.display_name}
                    <span className="ml-2 text-xs text-muted-foreground">
                      {row.key}
                    </span>
                  </button>
                </td>
                <td className="px-3 py-1.5 text-muted-foreground">
                  {SOURCE_LABELS[row.source] ?? row.source}
                </td>
                <td className="truncate px-3 py-1.5 font-mono text-xs text-muted-foreground">
                  {row.skills_dir ?? row.global_skills_dir ?? "—"}
                </td>
                <td className="px-3 py-1.5 tabular-nums">
                  {row.usages.length}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <aside className="w-72 shrink-0 overflow-auto border-l border-border xl:w-95">
        <header className="border-b border-border px-4 py-3">
          <h2 className="truncate text-sm font-semibold">
            {active.display_name}
          </h2>
          <dl className="mt-1 grid grid-cols-[auto_1fr] gap-x-2 text-xs text-muted-foreground">
            {active.skills_dir === undefined ? null : (
              <>
                <dt>项目目录</dt>
                <dd className="truncate font-mono">{active.skills_dir}</dd>
              </>
            )}
            {active.global_skills_dir === undefined ? null : (
              <>
                <dt>全局目录</dt>
                <dd className="truncate font-mono">
                  {active.global_skills_dir}
                </dd>
              </>
            )}
          </dl>
        </header>
        <UsageSection
          title="项目用法"
          empty="还没有项目用这个 agent 落技能"
          usages={projectUsages}
        />
        <UsageSection
          title="全局用法"
          empty="还没有技能装到全局技能目录"
          usages={globalUsages}
        />
      </aside>
    </div>
  );
}
