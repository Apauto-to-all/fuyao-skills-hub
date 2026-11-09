import type { ReactNode } from "react";
import { agentDisplayName } from "@/lib/agents";
import { FOCUS_RING } from "@/lib/focus-ring";
import type { AgentInfo } from "@/lib/ipc";
import { cn } from "@/lib/utils";

/**
 * agent 勾选组：候选清单 + 勾选集 + 切换；显示名统一经 lib 层的词表解析，
 * 行尾可挂一段补充说明（如该 agent 对应的技能目录）。
 */
export function AgentChecklist({
  legend,
  candidates,
  agents,
  selected,
  onToggle,
  hintOf,
}: {
  legend: string;
  /** 候选 agent 键，按传入顺序渲染 */
  candidates: readonly string[];
  /** agent 配置：解析显示名与行尾说明 */
  agents: readonly AgentInfo[] | undefined;
  /** 勾中的 agent 键 */
  selected: readonly string[];
  onToggle: (key: string) => void;
  /** 行尾补充说明；没有配置行的候选不显示 */
  hintOf?: (agent: AgentInfo) => ReactNode;
}) {
  return (
    <fieldset className="flex flex-col gap-1.5">
      <legend className="text-xs font-medium">{legend}</legend>
      <div className="max-h-40 overflow-auto rounded-md border border-border p-1">
        {candidates.map((key) => {
          const agent = agents?.find((row) => row.key === key);
          const hint = agent === undefined ? null : hintOf?.(agent);
          return (
            <label
              key={key}
              className={cn(
                "flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent",
                FOCUS_RING,
              )}
            >
              <input
                type="checkbox"
                checked={selected.includes(key)}
                onChange={() => onToggle(key)}
                className="size-3.5 accent-primary"
              />
              <span className="truncate">{agentDisplayName(agents, key)}</span>
              {hint ? (
                <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                  {hint}
                </span>
              ) : null}
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
