import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
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
import { toast } from "@/components/ui/toast";
import { api, isGuiError, type SkillDetail } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { FOCUS_RING } from "./parts";

/** 使用表单：目录选择 + Agent 多选 + 版本（跟随最新或固定一版） */
export function UseDialog({
  detail,
  open: visible,
  onOpenChange,
}: {
  detail: SkillDetail;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [project, setProject] = useState("");
  const [agents, setAgents] = useState<string[]>([]);
  const [version, setVersion] = useState("");
  const [error, setError] = useState<string | null>(null);
  const queryClient = useQueryClient();

  const agentList = useQuery({ queryKey: ["agents"], queryFn: api.agents });

  const pickProject = async () => {
    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择项目目录",
      });
      if (typeof picked === "string") setProject(picked);
    } catch (error) {
      toast.add({
        title: "无法打开目录选择器",
        description: isGuiError(error) ? error.message : String(error),
        type: "error",
      });
    }
  };

  const use = useMutation({
    scope: { id: "hub-write" },
    mutationFn: () =>
      api.attachSkill(
        detail.key,
        version === "" ? null : version,
        project,
        agents,
      ),
    onSuccess: (report) => {
      toast.add({
        title: `已接入 ${report.destinations.length} 处落点`,
        type: "success",
      });
      void queryClient.invalidateQueries({ queryKey: ["projects"] });
      void queryClient.invalidateQueries({ queryKey: ["agents"] });
      void queryClient.invalidateQueries({ queryKey: ["skill", detail.key] });
      void queryClient.invalidateQueries({ queryKey: ["skills"] });
      onOpenChange(false);
    },
    onError: (err) => {
      if (isGuiError(err) && err.code === "lock_held") {
        toast.add({
          title: "另一个 fsh 进程正在写，稍后重试",
          type: "error",
        });
        return;
      }
      setError(isGuiError(err) ? err.message : String(err));
    },
  });

  const toggleAgent = (key: string) =>
    setAgents((prev) =>
      prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key],
    );

  return (
    <Dialog open={visible} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>使用 {detail.key}</DialogTitle>
          <DialogDescription>
            把技能铺进项目的 agent 技能目录，并登记到项目清单
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-1.5">
          <span className="text-ui-sm font-medium">项目目录</span>
          <div className="flex items-center gap-1.5">
            <p className="min-w-0 flex-1 truncate rounded-md border border-border px-2 py-1.5 text-ui-sm text-muted-foreground">
              {project === "" ? "尚未选择" : project}
            </p>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void pickProject()}
            >
              选择…
            </Button>
          </div>
        </div>

        <fieldset className="flex flex-col gap-1.5">
          <legend className="text-ui-sm font-medium">Agent</legend>
          <div className="max-h-40 overflow-auto rounded-md border border-border p-1">
            {(agentList.data ?? []).map((agent) => (
              <label
                key={agent.key}
                className={cn(
                  "flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-ui-sm hover:bg-accent",
                  FOCUS_RING,
                )}
              >
                <input
                  type="checkbox"
                  checked={agents.includes(agent.key)}
                  onChange={() => toggleAgent(agent.key)}
                  className="size-3.5 accent-primary"
                />
                <span className="truncate">{agent.display_name}</span>
                <span className="ml-auto shrink-0 text-meta text-muted-foreground">
                  {agent.skills_dir}
                </span>
              </label>
            ))}
          </div>
          {agentList.isError ? (
            <p className="flex items-center gap-2 text-ui-sm text-destructive">
              读取 agent 失败：
              {isGuiError(agentList.error)
                ? agentList.error.message
                : String(agentList.error)}
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void agentList.refetch()}
              >
                重试
              </Button>
            </p>
          ) : null}
        </fieldset>

        <div className="flex flex-col gap-1.5">
          <label htmlFor="use-version" className="text-ui-sm font-medium">
            版本
          </label>
          <select
            id="use-version"
            value={version}
            onChange={(event) => setVersion(event.target.value)}
            className={cn(
              "h-8 rounded-md border border-border bg-background px-2 text-ui-sm",
              FOCUS_RING,
            )}
          >
            <option value="">跟随最新</option>
            {detail.timeline.map((entry) => (
              <option key={entry.version} value={entry.version}>
                v{entry.version}
              </option>
            ))}
          </select>
        </div>

        {error === null ? null : (
          <p className="text-ui-sm text-destructive">{error}</p>
        )}

        <DialogFooter>
          <Button
            size="sm"
            disabled={project === "" || agents.length === 0 || use.isPending}
            onClick={() => {
              setError(null);
              use.mutate();
            }}
          >
            {use.isPending ? "正在接入…" : "接入项目"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
