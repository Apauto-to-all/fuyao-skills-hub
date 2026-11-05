import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { Centered, ErrorBlock } from "./feedback";
import { FOCUS_RING } from "./library/parts";

const SOURCE_LABELS: Record<string, string> = {
  built_in: "内置",
  user: "用户覆盖",
};

type Props = {
  /** 命令面板跳 agent：seq 让同一个键第二次跳也能触发定位 */
  focusKey?: { key: string; seq: number } | null;
};

/** Agent 页：表格看数量和来源，检查器从 agent 反查用它的项目与技能 */
export function AgentsPage({ focusKey }: Props) {
  const [selected, setSelected] = useState<string | null>(null);
  const agents = useQuery({ queryKey: ["agents"], queryFn: api.agents });

  useEffect(() => {
    if (focusKey) setSelected(focusKey.key);
  }, [focusKey]);

  if (agents.isPending) return <Centered>正在读取 agent…</Centered>;
  if (agents.isError)
    return <ErrorBlock error={agents.error} onRetry={() => agents.refetch()} />;

  const rows = agents.data;
  const active = rows.find((row) => row.key === selected) ?? rows[0];

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
                  {row.skills_dir}
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
          <p className="text-xs text-muted-foreground">
            {active.usages.length} 处使用 · {active.skills_dir}
          </p>
        </header>
        {active.usages.length === 0 ? (
          <p className="px-4 py-3 text-xs text-muted-foreground">
            还没有项目用这个 agent 落技能
          </p>
        ) : (
          <ul>
            {active.usages.map((usage) => (
              <li
                key={`${usage.project}:${usage.key}`}
                className="border-b border-border/60 px-4 py-2"
              >
                <p className="truncate text-sm" title={usage.project}>
                  {usage.project}
                </p>
                <p className="truncate text-xs text-muted-foreground">
                  {usage.key} ·{" "}
                  {usage.version ? `v${usage.version}` : "跟随最新"}
                </p>
              </li>
            ))}
          </ul>
        )}
      </aside>
    </div>
  );
}
