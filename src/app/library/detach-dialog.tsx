import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { hubQueries } from "@/lib/hub-queries";
import { api, type SkillDetail, type SkillUsage } from "@/lib/ipc";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "../confirm-dialog";
import { FOCUS_RING, usageRowKey, usageTargetLabel } from "./parts";
import { UsageRecordPicker } from "./usage-picker";

/** 若干条记录 agent 名单的交集：core 要的是一份名单且必须是每个目标的子集 */
function intersectAgents(records: SkillUsage[]): string[] {
  if (records.length === 0) return [];
  return records
    .slice(1)
    .reduce<string[]>(
      (kept, record) => kept.filter((agent) => record.agents.includes(agent)),
      [...records[0].agents],
    );
}

/**
 * 解除多选对话框：列出该技能的全部使用记录（默认全选），勾中的记录折成
 * 「全局开关 + 项目路径列表」，连同勾中的 agent 一次调用撤掉。
 * agent 候选取勾中记录名单的交集并默认全选：交集必然合法，并集会被 core 拒绝。
 */
export function DetachDialog({
  detail,
  open,
  onOpenChange,
}: {
  detail: SkillDetail;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [checked, setChecked] = useState<string[]>(() =>
    detail.usage.map(usageRowKey),
  );
  // 用户主动取消过的 agent；候选变化后仍然记住，避免重算时又勾回来
  const [dropped, setDropped] = useState<string[]>([]);
  const agentList = useQuery(hubQueries.agents);

  // 详情载荷是项目段在前、全局段在后；对话框按全局在前重排，
  // 与使用页两段的展示顺序一致，勾选状态仍凭记录键，与排列无关
  const ordered = [
    ...detail.usage.filter((usage) => usage.scope === "global"),
    ...detail.usage.filter((usage) => usage.scope === "project"),
  ];

  // 显示名取自 agent 配置；配置里没有这个键（用户删了覆盖）时退回键本身
  const nameOf = (agentKey: string) =>
    agentList.data?.find((agent) => agent.key === agentKey)?.display_name ??
    agentKey;

  const selected = detail.usage.filter((usage) =>
    checked.includes(usageRowKey(usage)),
  );
  const candidates = intersectAgents(selected);
  const withdrawn = candidates.filter((agent) => !dropped.includes(agent));
  const untouched = detail.usage.length - selected.length;

  const detach = useHubMutation({
    mutationFn: ({
      records,
      agents,
    }: {
      records: SkillUsage[];
      agents: string[];
    }) =>
      api.detachSkill(
        detail.key,
        records.some((record) => record.scope === "global"),
        records.flatMap((record) =>
          record.scope === "project" ? [record.project] : [],
        ),
        agents,
      ),
    success: (report) => `已撤 ${report.targets.length} 处记录`,
    failure: "解除失败",
    onSuccess: () => onOpenChange(false),
  });

  // 三种禁用原因共用同一个禁用位，说明文案各不相同
  const disabled = selected.length === 0 || withdrawn.length === 0;
  const note =
    selected.length === 0
      ? "至少勾选一条使用记录"
      : candidates.length === 0
        ? "这几处没有共同 agent，请逐处解除"
        : withdrawn.length === 0
          ? "至少勾选一个 agent 才能解除"
          : undefined;

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title="解除使用"
      description={describe({
        detail,
        selected,
        candidates,
        withdrawn,
        untouched,
        nameOf,
      })}
      confirmLabel="解除"
      destructive
      pending={detach.isPending}
      confirmDisabled={disabled}
      note={note}
      onConfirm={() => detach.mutate({ records: selected, agents: withdrawn })}
    >
      <div className="flex flex-col gap-3">
        <UsageRecordPicker
          usage={ordered}
          checked={checked}
          legend="要撤的使用记录"
          onToggle={(key) =>
            setChecked((prev) =>
              prev.includes(key)
                ? prev.filter((k) => k !== key)
                : [...prev, key],
            )
          }
        />
        {candidates.length > 0 ? (
          <fieldset className="flex flex-col gap-1.5">
            <legend className="text-xs font-medium">要撤的 agent</legend>
            <div className="max-h-40 overflow-auto rounded-md border border-border p-1">
              {candidates.map((agentKey) => (
                <label
                  key={agentKey}
                  className={cn(
                    "flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent",
                    FOCUS_RING,
                  )}
                >
                  <input
                    type="checkbox"
                    checked={!dropped.includes(agentKey)}
                    onChange={() =>
                      setDropped((prev) =>
                        prev.includes(agentKey)
                          ? prev.filter((k) => k !== agentKey)
                          : [...prev, agentKey],
                      )
                    }
                    className="size-3.5 accent-primary"
                  />
                  <span className="truncate">{nameOf(agentKey)}</span>
                </label>
              ))}
            </div>
          </fieldset>
        ) : null}
      </div>
    </ConfirmDialog>
  );
}

/** 确认文案：全选交集时写撤几处留几处，只撤部分 agent 时点名谁留下 */
function describe({
  detail,
  selected,
  candidates,
  withdrawn,
  untouched,
  nameOf,
}: {
  detail: SkillDetail;
  selected: SkillUsage[];
  candidates: string[];
  withdrawn: string[];
  untouched: number;
  nameOf: (agentKey: string) => string;
}): string {
  if (selected.length === 0) {
    return "勾中的记录一次撤掉落点与登记；不勾就没有可撤的目标。";
  }
  const targets = selected.map(usageTargetLabel).join("、");
  const names = withdrawn.map(nameOf).join("、");

  // 交集为空时给不出可撤的名单，说明要走逐处解除
  if (candidates.length === 0) {
    return `${targets} 这 ${selected.length} 处记录没有共同 agent，一次调用撤不了；请逐处解除。`;
  }
  // agent 全被取消时名单为空，不能拼出「撤  的落点」这种坏句子
  if (withdrawn.length === 0) {
    return `这 ${selected.length} 处记录上的 agent 一个都没勾；勾上要撤的 agent 才能解除。`;
  }
  if (withdrawn.length === candidates.length) {
    return `撤 ${targets} 这 ${selected.length} 处记录上的落点与登记（agent：${names}）；本技能另外 ${untouched} 处记录原样留下。`;
  }
  const kept = candidates
    .filter((agent) => !withdrawn.includes(agent))
    .map(nameOf)
    .join("、");
  return `只撤 ${detail.key} 在 ${targets} 的 ${names} 落点与登记，${kept}的落点与登记原样留下。被其他 agent 共用的目录会保留。`;
}
