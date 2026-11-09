import type { ReactNode } from "react";

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
