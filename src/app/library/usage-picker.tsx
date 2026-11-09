import { FOCUS_RING } from "@/lib/focus-ring";
import type { SkillUsage } from "@/lib/ipc";
import { scopeLabel, usageRowKey, usageVersionLabel } from "@/lib/registry";
import { cn } from "@/lib/utils";

/**
 * 使用记录勾选列表：一条记录一行，作用域标记 + 目标 + 登记版本 + agent 名单。
 * 版本切换与解除两个对话框共用这一件；勾选状态与记录顺序都由调用方持有，
 * 换记录或换目标重挂后回到调用方给的初始勾选。
 */
export function UsageRecordPicker({
  usage,
  checked,
  legend,
  onToggle,
}: {
  /** 要列出的使用记录，按传入顺序渲染 */
  usage: SkillUsage[];
  /** 已勾中的记录键（usageRowKey） */
  checked: string[];
  /** 勾选组的标题 */
  legend: string;
  onToggle: (key: string) => void;
}) {
  return (
    <fieldset className="flex flex-col gap-1.5">
      <legend className="text-xs font-medium">{legend}</legend>
      <div className="max-h-56 overflow-auto rounded-md border border-border p-1">
        {usage.map((record) => {
          const key = usageRowKey(record);
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
                checked={checked.includes(key)}
                onChange={() => onToggle(key)}
                className="size-3.5 accent-primary"
              />
              <span className="shrink-0 rounded border border-border px-1 text-muted-foreground">
                {scopeLabel(record.scope)}
              </span>
              {record.scope === "project" ? (
                <span
                  className="min-w-0 flex-1 truncate"
                  title={record.project}
                >
                  {record.project}
                </span>
              ) : (
                <span className="flex-1" />
              )}
              <span className="shrink-0 text-muted-foreground">
                {usageVersionLabel(record.version)}
              </span>
              <span className="shrink-0 text-muted-foreground">
                {record.agents.join("、")}
              </span>
            </label>
          );
        })}
      </div>
    </fieldset>
  );
}
