import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { hubQueries } from "@/lib/hub-queries";
import { api, type DestinationView, type ProjectSkillView } from "@/lib/ipc";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "./confirm-dialog";
import { FOCUS_RING } from "./library/parts";

/** 一次解除的目标：项目路径 + 这条记录的键与 agent 名单（勾选与文案都取自它） */
type DetachTarget = {
  project: string;
  skill: Pick<ProjectSkillView, "key" | "agents">;
};

/**
 * 解除确认框：列出这条记录铺给的全部 agent（默认全选），取消勾选即只撤勾上的
 * 那些；一个都不勾时确认按钮禁用。
 */
function DetachDialog({
  target,
  pending,
  onClose,
  onConfirm,
}: {
  target: DetachTarget | null;
  pending: boolean;
  onClose: () => void;
  onConfirm: (agents: string[]) => void;
}) {
  const registered = target?.skill.agents ?? [];
  const [checked, setChecked] = useState<string[]>(registered);
  const agentList = useQuery(hubQueries.agents);

  // 显示名取自 agent 配置；配置里没有这个键（用户删了覆盖）时退回键本身
  const nameOf = (agentKey: string) =>
    agentList.data?.find((agent) => agent.key === agentKey)?.display_name ??
    agentKey;

  const withdrawn = registered.filter((agentKey) => checked.includes(agentKey));
  const kept = registered.filter((agentKey) => !checked.includes(agentKey));
  const names = withdrawn.map(nameOf).join("、");
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
            ? `从 ${target.project} 撤回 ${target.skill.key} 的全部落点与登记记录（agent：${names}）。`
            : `从 ${target.project} 只撤 ${target.skill.key} 在 ${names} 的落点与登记，${kept.map(nameOf).join("、")}的落点与登记原样留下。被其他 agent 共用的目录会保留。`
      }
      confirmLabel="解除"
      pending={pending}
      confirmDisabled={withdrawn.length === 0}
      note={withdrawn.length === 0 ? "至少勾选一个 agent 才能解除" : undefined}
      onConfirm={() => onConfirm(withdrawn)}
    >
      {target === null ? null : (
        <fieldset className="flex flex-col gap-1.5">
          <legend className="text-xs font-medium">要撤的 agent</legend>
          <div className="max-h-40 overflow-auto rounded-md border border-border p-1">
            {target.skill.agents.map((agentKey) => (
              <label
                key={agentKey}
                className={cn(
                  "flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent",
                  FOCUS_RING,
                )}
              >
                <input
                  type="checkbox"
                  checked={checked.includes(agentKey)}
                  onChange={() =>
                    setChecked((prev) =>
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
      )}
    </ConfirmDialog>
  );
}

/**
 * 「解除使用」的共享流程：确认文案、agent 勾选、mutation 与 pending 状态
 * 只写一次，项目页与技能详情使用页共用；request 传入目标即弹确认框。
 */
export function useDetachFlow() {
  const [pending, setPending] = useState<DetachTarget | null>(null);

  const unuse = useHubMutation({
    mutationFn: ({
      target,
      agents,
    }: {
      target: DetachTarget;
      agents: string[];
    }) => api.detachSkill(target.skill.key, false, [target.project], agents),
    success: "已解除使用",
    failure: "解除失败",
    onSuccess: () => setPending(null),
  });

  return {
    /** 请求解除一个目标：弹确认框，确认后才调桥 */
    request: setPending,
    dialog: (
      <DetachDialog
        // 换目标即重挂，勾选回到这条记录的默认全选
        key={pending === null ? "" : `${pending.project}:${pending.skill.key}`}
        target={pending}
        pending={unuse.isPending}
        onClose={() => setPending(null)}
        onConfirm={(agents) =>
          pending !== null && unuse.mutate({ target: pending, agents })
        }
      />
    ),
  };
}

/** 落点列表：圆点 + 路径，缺失的整行标出（两处入口共用一份行为） */
export function DestinationList({
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
