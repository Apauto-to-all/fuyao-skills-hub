import { useState } from "react";
import { api, type SkillDetail, type SkillUsage } from "@/lib/ipc";
import { upgradeToLabel, usageRowKey, usageTargets } from "@/lib/registry";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { ConfirmDialog } from "../confirm-dialog";
import { UsageRecordPicker } from "./usage-picker";

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
    mutationFn: (selected: SkillUsage[]) => {
      const targets = usageTargets(selected);
      return api.upgradeSkill(
        detail.key,
        version,
        targets.global,
        targets.projects,
      );
    },
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
      title={upgradeToLabel(version)}
      description="勾中的记录一次切到这一版；不新建记录、不改 agent 名单。"
      confirmLabel="切换"
      pending={upgrade.isPending}
      confirmDisabled={selected.length === 0}
      note={selected.length === 0 ? "至少勾选一条使用记录" : undefined}
      onConfirm={() => upgrade.mutate(selected)}
    >
      <UsageRecordPicker
        usage={detail.usage}
        checked={checked}
        legend="要换版本的记录"
        onToggle={(key) =>
          setChecked((prev) =>
            prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key],
          )
        }
      />
    </ConfirmDialog>
  );
}
