import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { EmptyState } from "@/components/empty-state";
import { Button } from "@/components/ui/button";
import { FOCUS_RING } from "@/lib/focus-ring";
import { relativeTime } from "@/lib/format";
import { hubQueries } from "@/lib/hub-queries";
import type { DestinationView, ProjectView } from "@/lib/ipc";
import { usageTargets, usageVersionLabel } from "@/lib/registry";
import { cn } from "@/lib/utils";
import { useDetachFlow } from "./detach-flow";
import { Centered, ErrorBlock } from "./feedback";

/** 项目页：表格看占用，检查器看一个项目里的每条技能与落点，可就地解除 */
export function ProjectsPage() {
  const [selected, setSelected] = useState<string | null>(null);
  const projects = useQuery(hubQueries.projects);
  const detach = useDetachFlow();

  if (projects.isPending) return <Centered>正在读取项目…</Centered>;
  if (projects.isError)
    return (
      <ErrorBlock error={projects.error} onRetry={() => projects.refetch()} />
    );

  const rows = projects.data;
  if (rows.length === 0) {
    return (
      <EmptyState
        title="还没有项目用过技能"
        hint="技能被使用后才会出现在这里：在库页详情点「使用…」"
      />
    );
  }

  const active = rows.find((row) => row.path === selected) ?? rows[0];
  const badCount = (row: ProjectView) =>
    row.skills
      .flatMap((skill) => skill.destinations)
      .filter((dest) => !dest.exists).length;

  return (
    <div className="flex h-full min-h-0">
      <div className="min-w-0 flex-1 overflow-auto">
        <table className="w-full table-fixed border-collapse text-sm">
          <colgroup>
            <col />
            <col className="w-16" />
            <col className="w-20" />
            <col className="w-24" />
          </colgroup>
          <thead>
            <tr className="border-b border-border text-left text-xs text-muted-foreground">
              <th scope="col" className="px-4 py-2 font-medium">
                项目
              </th>
              <th scope="col" className="px-3 py-2 font-medium tabular-nums">
                技能
              </th>
              <th scope="col" className="px-3 py-2 font-medium tabular-nums">
                落点异常
              </th>
              <th scope="col" className="px-3 py-2 font-medium">
                最近使用
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => {
              const bad = badCount(row);
              return (
                <tr
                  key={row.path}
                  className={cn(
                    "border-b border-border/60",
                    row.path === active.path && "bg-brand/5",
                  )}
                >
                  <td className="max-w-96 px-4 py-1.5">
                    <button
                      type="button"
                      onClick={() => setSelected(row.path)}
                      aria-current={
                        row.path === active.path ? "true" : undefined
                      }
                      className={cn(
                        "block max-w-full truncate text-left",
                        FOCUS_RING,
                      )}
                      title={row.path}
                    >
                      {row.path}
                    </button>
                  </td>
                  <td className="px-3 py-1.5 tabular-nums">
                    {row.skills.length}
                  </td>
                  <td className="px-3 py-1.5 tabular-nums">
                    {bad === 0 ? (
                      <span className="text-muted-foreground">0</span>
                    ) : (
                      <span className="flex items-center gap-1.5">
                        <span
                          aria-hidden="true"
                          className="size-1.5 rounded-full bg-destructive"
                        />
                        {bad}
                      </span>
                    )}
                  </td>
                  <td className="px-3 py-1.5 text-muted-foreground">
                    {relativeTime(row.last_used)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <aside className="w-72 shrink-0 overflow-auto border-l border-border xl:w-95">
        <header className="border-b border-border px-4 py-3">
          <h2 className="truncate text-sm font-semibold" title={active.path}>
            {active.path}
          </h2>
          <p className="text-xs text-muted-foreground">
            {active.skills.length} 条技能 · {relativeTime(active.last_used)}
          </p>
        </header>
        {active.skills.length === 0 ? (
          <p className="px-4 py-3 text-xs text-muted-foreground">
            这个项目没有登记技能
          </p>
        ) : (
          <ul>
            {active.skills.map((skill) => (
              <li
                key={skill.key}
                className="flex flex-col gap-1 border-b border-border/60 px-4 py-2"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-sm font-medium">
                    {skill.key}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      detach.request({
                        key: skill.key,
                        agents: skill.agents,
                        targets: usageTargets([
                          { scope: "project", project: active.path },
                        ]),
                      })
                    }
                  >
                    解除
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  {usageVersionLabel(skill.version)} · {skill.agents.join("、")}{" "}
                  · {skill.link_label}
                </p>
                <DestinationList destinations={skill.destinations} />
              </li>
            ))}
          </ul>
        )}
      </aside>

      {detach.dialog}
    </div>
  );
}

/** 落点列表：圆点 + 路径，缺失的整行标出 */
function DestinationList({
  destinations,
}: {
  destinations: DestinationView[];
}) {
  return (
    <ul className="flex flex-col gap-0.5">
      {destinations.map((dest) => (
        <li key={dest.path} className="flex items-center gap-1.5 text-xs">
          <span
            aria-hidden="true"
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              dest.exists ? "bg-success" : "bg-destructive",
            )}
          />
          <span
            className={cn(
              "truncate",
              dest.exists ? "text-muted-foreground" : "text-foreground",
            )}
            title={dest.path}
          >
            {dest.exists ? dest.path : `缺失：${dest.path}`}
          </span>
        </li>
      ))}
    </ul>
  );
}
