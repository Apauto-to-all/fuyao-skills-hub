import { useState } from "react";
import { api } from "@/lib/ipc";
import type { WithdrawRequest } from "@/lib/registry";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { SingleTargetDetachDialog } from "./single-target-detach-dialog";

/**
 * 「解除使用」的共享流程：确认文案、agent 勾选、mutation 与 pending 状态
 * 只写一次，项目页与技能详情使用页共用；request 传入目标即弹确认框。
 */
export function useDetachFlow() {
  const [pending, setPending] = useState<WithdrawRequest | null>(null);

  const unuse = useHubMutation({
    mutationFn: ({
      request,
      agents,
    }: {
      request: WithdrawRequest;
      agents: string[] | undefined;
    }) =>
      api.detachSkill(
        request.key,
        request.targets.global,
        request.targets.projects,
        agents,
      ),
    success: "已解除使用",
    failure: "解除失败",
    onSuccess: () => setPending(null),
  });

  return {
    /** 请求解除一个目标：弹确认框，确认后才调桥 */
    request: setPending,
    dialog: (
      <SingleTargetDetachDialog
        // 换目标即重挂，勾选回到这条记录的默认全选
        key={
          pending === null
            ? ""
            : `${pending.targets.global}:${pending.targets.projects.join("|")}:${pending.key}`
        }
        target={pending}
        pending={unuse.isPending}
        onClose={() => setPending(null)}
        onConfirm={(agents) =>
          pending !== null && unuse.mutate({ request: pending, agents })
        }
      />
    ),
  };
}
