import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toast } from "@/components/ui/toast";
import { api, isGuiError, type SkillDetail } from "@/lib/ipc";

/** 发版框：输入版本号；只有「等于最新版本」的号才给「将覆盖」的二次确认 */
export function ReleaseDialog({
  detail,
  open,
  onOpenChange,
}: {
  detail: SkillDetail;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  // 空白起步：预填旧号会让默认流程落进「已存在 → 覆盖」两步
  const [version, setVersion] = useState("");
  const [warned, setWarned] = useState(false);
  const queryClient = useQueryClient();
  const target = version.trim();
  const exists = detail.timeline.some((entry) => entry.version === target);
  // 覆盖只对最新版成立：更旧的版本是项目引用的锚点，core 也会拒绝
  const overwrite = exists && target === detail.latest_version;
  const outdated = exists && !overwrite;

  const release = useMutation({
    scope: { id: "hub-write" },
    mutationFn: () => api.releaseSkill(detail.key, target, overwrite),
    onSuccess: (published) => {
      toast.add({ title: `已发布 v${published}`, type: "success" });
      void queryClient.invalidateQueries({ queryKey: ["skills"] });
      void queryClient.invalidateQueries({ queryKey: ["skill", detail.key] });
      onOpenChange(false);
    },
    onError: (error) => {
      toast.add({
        title:
          isGuiError(error) && error.code === "lock_held"
            ? "另一个 fsh 进程正在写，稍后重试"
            : "发版失败",
        description: isGuiError(error) ? error.message : String(error),
        type: "error",
      });
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{warned ? `覆盖版本 v${target}` : "发版"}</DialogTitle>
          <DialogDescription>
            {warned
              ? `用当前工作副本重写 ${detail.key} 的 v${target} 标签与提交`
              : "把当前工作副本（含未提交改动）发成新版本"}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-1.5">
          <label htmlFor="release-version" className="text-ui-sm font-medium">
            版本号
          </label>
          <Input
            id="release-version"
            value={version}
            onChange={(event) => {
              setVersion(event.target.value);
              setWarned(false);
            }}
            placeholder="如 1.2.0，三段纯数字"
            className="font-mono"
          />
        </div>

        {overwrite && warned ? (
          <p className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-ui-sm">
            版本 v{target} 已存在，将覆盖该版本的提交与内容
          </p>
        ) : null}
        {outdated ? (
          <p className="rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-ui-sm">
            只能覆盖最新版 v{detail.latest_version ?? "—"}，更旧的版本不能重发
          </p>
        ) : null}

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={release.isPending}
          >
            取消
          </Button>
          <Button
            onClick={() => {
              if (overwrite && !warned) {
                setWarned(true);
                return;
              }
              release.mutate();
            }}
            disabled={target === "" || outdated || release.isPending}
          >
            {release.isPending
              ? "发布中…"
              : overwrite
                ? warned
                  ? "覆盖并发布"
                  : "下一步"
                : "发布"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
