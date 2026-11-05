import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { Centered, ErrorBlock } from "./feedback";
import { FOCUS_RING } from "./library/parts";

const SOURCE_LABELS: Record<string, string> = {
  built_in: "内置",
  user: "用户覆盖",
};

/** Agent 页：表格看数量和来源，检查器从 agent 反查用它的项目与技能 */
export function AgentsPage() {
  const [selected, setSelected] = useState<string | null>(null);
  const agents = useQuery({ queryKey: ["agents"], queryFn: api.agents });

  if (agents.isPending) return <Centered>正在读取 agent…</Centered>;
  if (agents.isError)
    return <ErrorBlock error={agents.error} onRetry={() => agents.refetch()} />;

  const rows = agents.data;
  const active = rows.find((row) => row.key === selected) ?? rows[0];

  return (
    <div className="flex h-full min-h-0">
      <div className="min-w-0 flex-1 overflow-auto">
        <table className="w-full border-collapse text-ui">
          <thead>
            <tr className="border-b border-border text-left text-meta text-muted-foreground">
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
                className={cn(
                  "border-b border-border/60",
                  row.key === active.key && "bg-brand/5",
                )}
              >
                <td className="px-4 py-1.5">
                  <button
                    type="button"
                    onClick={() => setSelected(row.key)}
                    aria-current={row.key === active.key ? "true" : undefined}
                    className={cn("text-left", FOCUS_RING)}
                  >
                    {row.display_name}
                    <span className="ml-2 text-meta text-muted-foreground">
                      {row.key}
                    </span>
                  </button>
                </td>
                <td className="px-3 py-1.5 text-muted-foreground">
                  {SOURCE_LABELS[row.source] ?? row.source}
                </td>
                <td className="px-3 py-1.5 font-mono text-meta text-muted-foreground">
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

      <aside className="w-95 shrink-0 overflow-auto border-l border-border">
        <header className="border-b border-border px-4 py-3">
          <h2 className="truncate text-ui font-semibold">
            {active.display_name}
          </h2>
          <p className="text-meta text-muted-foreground">
            {active.usages.length} 处使用 · {active.skills_dir}
          </p>
        </header>
        {active.usages.length === 0 ? (
          <p className="px-4 py-3 text-ui-sm text-muted-foreground">
            还没有项目用这个 agent 落技能
          </p>
        ) : (
          <table className="w-full border-collapse text-ui">
            <thead>
              <tr className="border-b border-border text-left text-meta text-muted-foreground">
                <th scope="col" className="px-4 py-1.5 font-medium">
                  项目
                </th>
                <th scope="col" className="px-3 py-1.5 font-medium">
                  技能
                </th>
                <th scope="col" className="px-3 py-1.5 font-medium">
                  版本
                </th>
              </tr>
            </thead>
            <tbody>
              {active.usages.map((usage) => (
                <tr
                  key={`${usage.project}:${usage.key}`}
                  className="border-b border-border/60"
                >
                  <td
                    className="max-w-40 truncate px-4 py-1.5"
                    title={usage.project}
                  >
                    {usage.project}
                  </td>
                  <td className="max-w-32 truncate px-3 py-1.5">{usage.key}</td>
                  <td className="px-3 py-1.5 tabular-nums text-muted-foreground">
                    {usage.version === null ? "跟随最新" : `v${usage.version}`}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </aside>
    </div>
  );
}
