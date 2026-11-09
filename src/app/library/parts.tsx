import type { ReactNode } from "react";
import { FOCUS_RING } from "@/lib/focus-ring";
import type { SkillSummary } from "@/lib/ipc";
import { cn } from "@/lib/utils";

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
