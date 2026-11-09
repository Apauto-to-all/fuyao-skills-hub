import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { AgentChecklist } from "@/components/agent-checklist";
import { agentDisplayName, splitAgents } from "@/lib/agents";
import { hubQueries } from "@/lib/hub-queries";
import type { WithdrawRequest } from "@/lib/registry";
import { GLOBAL_TARGET_LABEL, keptAgentsNote } from "@/lib/registry";
import { ConfirmDialog } from "./confirm-dialog";

/**
 * 单目标解除确认框：列出这条记录铺给的全部 agent（默认全选）；
 * 全选走缺省整撤（不传名单），取消勾选即只撤勾上的那些；一个都不勾时确认按钮禁用。
 */
export function SingleTargetDetachDialog({
  target,
  pending,
  onClose,
  onConfirm,
}: {
  target: WithdrawRequest | null;
  pending: boolean;
  onClose: () => void;
  onConfirm: (agents: string[] | undefined) => void;
}) {
  const registered = target?.agents ?? [];
  const [checked, setChecked] = useState<string[]>(registered);
  const agentList = useQuery(hubQueries.agents);

  // 目标的人读名：全局记录写「全局」，项目记录写路径
  const where =
    target === null
      ? ""
      : target.targets.global
        ? GLOBAL_TARGET_LABEL
        : target.targets.projects.join("、");

  const { withdrawn, kept } = splitAgents(registered, checked);
  const nameOf = (agentKey: string) =>
    agentDisplayName(agentList.data, agentKey);
  const names = withdrawn.map(nameOf).join("、");
  const keptNames = kept.map(nameOf).join("、");
  const all = withdrawn.length === registered.length;

  return (
    <ConfirmDialog
      open={target !== null}
      onOpenChange={(open) => !open && onClose()}
      title="解除使用"
      description={
        target === null
          ? ""
          : all
            ? `从 ${where} 撤回 ${target.key} 的全部落点与登记记录（agent：${names}）。`
            : `从 ${where} 只撤 ${target.key} 在 ${names} 的落点与登记，${keptAgentsNote(keptNames)}`
      }
      confirmLabel="解除"
      pending={pending}
      confirmDisabled={withdrawn.length === 0}
      note={withdrawn.length === 0 ? "至少勾选一个 agent 才能解除" : undefined}
      // 全选时省缺名单：桥按整体撤回处理
      onConfirm={() => onConfirm(all ? undefined : withdrawn)}
    >
      {target === null ? null : (
        <AgentChecklist
          legend="要撤的 agent"
          candidates={registered}
          agents={agentList.data}
          selected={checked}
          onToggle={(agentKey) =>
            setChecked((prev) =>
              prev.includes(agentKey)
                ? prev.filter((key) => key !== agentKey)
                : [...prev, agentKey],
            )
          }
        />
      )}
    </ConfirmDialog>
  );
}
