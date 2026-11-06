import { useState } from "react";
import { api, type DestinationView } from "@/lib/ipc";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "./confirm-dialog";

type DetachTarget = { project: string; key: string };

/**
 * 「解除使用」的共享流程：确认文案、mutation 与 pending 状态只写
 * 一次，项目页与技能详情使用页共用；request 传入目标即弹确认框。
 */
export function useDetachFlow() {
  const [pending, setPending] = useState<DetachTarget | null>(null);

  const unuse = useHubMutation({
    mutationFn: (target: DetachTarget) =>
      api.detachSkill(target.key, target.project),
    success: "已解除使用",
    failure: "解除失败",
    onSuccess: () => setPending(null),
  });

  return {
    /** 请求解除一个目标：弹确认框，确认后才调桥 */
    request: setPending,
    isPending: unuse.isPending,
    dialog: (
      <ConfirmDialog
        open={pending !== null}
        onOpenChange={(open) => !open && setPending(null)}
        title="解除使用"
        description={
          pending === null
            ? ""
            : `从 ${pending.project} 撤回 ${pending.key} 的全部落点与登记记录。`
        }
        confirmLabel="解除"
        pending={unuse.isPending}
        onConfirm={() => pending !== null && unuse.mutate(pending)}
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
