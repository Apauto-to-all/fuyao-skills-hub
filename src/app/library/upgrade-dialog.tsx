import { useState } from "react";
import { api, type SkillDetail, type SkillUsage } from "@/lib/ipc";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "../confirm-dialog";
import { FOCUS_RING, usageRowKey, usageVersionLabel } from "./parts";

/**
 * 版本切换确认框：列出该技能的全部使用记录（默认全选），确认后一次调用
 * 把勾中的全局目标与项目一起切到这一版；一条都不勾时确认按钮禁用。
 * 凭记录键勾选，换版本重挂后回到默认全选。
 */
export function UpgradeDialog({
  detail,
  version,
  open,
  onOpenChange,
}: {
  detail: SkillDetail;
  /** 要切到的版本；框关闭时是空串占位，此时提交不了 */
  version: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [checked, setChecked] = useState<string[]>(() =>
    detail.usage.map(usageRowKey),
  );

  const upgrade = useHubMutation({
    mutationFn: (selected: SkillUsage[]) =>
      api.upgradeSkill(
        detail.key,
        version,
        selected.some((usage) => usage.scope === "global"),
        selected.flatMap((usage) =>
          usage.scope === "project" ? [usage.project] : [],
        ),
      ),
    success: (report) => `已把 ${report.targets.length} 处记录切到 v${version}`,
    failure: "切换版本失败",
    onSuccess: () => onOpenChange(false),
  });

  const selected = detail.usage.filter((usage) =>
    checked.includes(usageRowKey(usage)),
  );

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={`切换到 v${version}`}
      description="勾中的记录一次切到这一版；不新建记录、不改 agent 名单。"
      confirmLabel="切换"
      pending={upgrade.isPending}
      confirmDisabled={selected.length === 0}
      note={selected.length === 0 ? "至少勾选一条使用记录" : undefined}
      onConfirm={() => upgrade.mutate(selected)}
    >
      <fieldset className="flex flex-col gap-1.5">
        <legend className="text-xs font-medium">要换版本的记录</legend>
        <div className="max-h-56 overflow-auto rounded-md border border-border p-1">
          {detail.usage.map((usage) => {
            const key = usageRowKey(usage);
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
                  onChange={() =>
                    setChecked((prev) =>
                      prev.includes(key)
                        ? prev.filter((k) => k !== key)
                        : [...prev, key],
                    )
                  }
                  className="size-3.5 accent-primary"
                />
                <span className="shrink-0 rounded border border-border px-1 text-muted-foreground">
                  {usage.scope === "project" ? "项目" : "全局"}
                </span>
                {usage.scope === "project" ? (
                  <span
                    className="min-w-0 flex-1 truncate"
                    title={usage.project}
                  >
                    {usage.project}
                  </span>
                ) : (
                  <span className="flex-1" />
                )}
                <span className="shrink-0 text-muted-foreground">
                  {usageVersionLabel(usage.version)}
                </span>
                <span className="shrink-0 text-muted-foreground">
                  {usage.agents.join("、")}
                </span>
              </label>
            );
          })}
        </div>
      </fieldset>
    </ConfirmDialog>
  );
}
