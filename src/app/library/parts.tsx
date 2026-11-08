import type { ReactNode } from "react";
import type { SkillSummary, SkillUsage } from "@/lib/ipc";
import { cn } from "@/lib/utils";

/** 统一键盘焦点环：2px ring，全站可聚焦控件共用 */
export const FOCUS_RING =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50";

export type ArchiveFilter = "all" | "active" | "archived";
export type SortMode = "updated" | "name";

export type Filters = {
  query: string;
  archive: ArchiveFilter;
  tags: string[];
  sort: SortMode;
};

export const ARCHIVE_LABELS: { value: ArchiveFilter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "active", label: "活跃" },
  { value: "archived", label: "已归档" },
];

/** 筛选与排序：只作用于已取回的清单，不额外发请求 */
export function selectRows(
  rows: SkillSummary[],
  filters: Filters,
): SkillSummary[] {
  const query = filters.query.trim().toLowerCase();
  const selected = rows.filter((row) => {
    if (filters.archive === "active" && row.archived) return false;
    if (filters.archive === "archived" && !row.archived) return false;
    if (!filters.tags.every((tag) => row.tags.includes(tag))) return false;
    if (query === "") return true;
    return [row.key, row.description ?? "", ...row.tags].some((value) =>
      value.toLowerCase().includes(query),
    );
  });
  return selected.sort((a, b) =>
    filters.sort === "name"
      ? a.key.localeCompare(b.key)
      : (b.updated ?? 0) - (a.updated ?? 0),
  );
}

/** 版本说法：跟最新时如实写「跟随最新」，不冒充某个 tag */
export function usageVersionLabel(version: string | null): string {
  return version === null ? "跟随最新" : `v${version}`;
}

/** 一行记录的目标：项目记录写路径，全局记录写「全局」 */
export function usageTargetLabel(usage: SkillUsage): string {
  return usage.scope === "project" ? usage.project : "全局";
}

/** 行身份：作用域 + 目标 + 登记版本 + agent 名单，同一记录不会重复出行 */
export function usageRowKey(usage: SkillUsage): string {
  const target = usage.scope === "project" ? usage.project : "";
  return `${usage.scope}:${target}:${usage.version ?? "latest"}:${usage.agents.join(",")}`;
}

/** 标签 chip：列表筛选条与详情概览共用一份样式与交互 */
export function TagChip({
  tag,
  active,
  onToggle,
}: {
  tag: string;
  active: boolean;
  onToggle: (tag: string) => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={() => onToggle(tag)}
      className={cn(
        "rounded border px-1.5 py-0.5 text-xs transition-colors duration-100",
        FOCUS_RING,
        active
          ? "border-brand/40 bg-brand/10 text-foreground"
          : "border-border text-muted-foreground hover:text-foreground",
      )}
    >
      #{tag}
    </button>
  );
}

export function Pill({
  children,
  tone = "default",
}: {
  children: ReactNode;
  tone?: "default" | "warning";
}) {
  return (
    <span
      className={cn(
        "rounded border px-1.5 py-0.5 text-xs tabular-nums",
        tone === "warning"
          ? "border-warning/40 bg-warning/10 text-foreground"
          : "border-border text-muted-foreground",
      )}
    >
      {children}
    </span>
  );
}

export function StatCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-md border border-border px-3 py-2">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 text-lg font-semibold tabular-nums">{value}</dd>
    </div>
  );
}

/** 空态：三种清单一律同一骨架（标题 + 说明 + 可选动作） */
export function EmptyState({
  title,
  hint,
  action,
}: {
  title: string;
  hint: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center gap-1.5 px-6 py-10 text-center">
      <p className="text-sm font-medium">{title}</p>
      <p className="text-xs text-muted-foreground">{hint}</p>
      {action}
    </div>
  );
}
