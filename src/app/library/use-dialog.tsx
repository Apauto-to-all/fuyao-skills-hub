import { useQuery } from "@tanstack/react-query";
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
import { hubQueries } from "@/lib/hub-queries";
import { api, isGuiError, type SkillDetail } from "@/lib/ipc";
import { pickDirectory } from "@/lib/pick-directory";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { cn } from "@/lib/utils";
import { FOCUS_RING } from "./parts";

/** 使用表单：目标（项目目录 / 全局，可并存）+ Agent 多选 + 版本（跟随最新或固定一版） */
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
  const [global, setGlobal] = useState(false);
  const [agents, setAgents] = useState<string[]>([]);
  const [version, setVersion] = useState("");

  const agentList = useQuery(hubQueries.agents);

  const pickProject = async () => {
    const picked = await pickDirectory("选择项目目录");
    if (picked !== null) setProject(picked);
  };

  const use = useHubMutation({
    mutationFn: () =>
      api.attachSkill(
        detail.key,
        version === "" ? null : version,
        global,
        project === "" ? null : project,
        agents,
      ),
    success: (report) =>
      `已接入 ${report.targets.reduce((sum, t) => sum + t.destinations.length, 0)} 处落点`,
    failure: "接入失败",
    onSuccess: () => onOpenChange(false),
  });

  const toggleAgent = (key: string) =>
    setAgents((prev) =>
      prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key],
    );

  // 一个目标都不给时后端会报 no_target，这里提前拦下并说明原因
  const noTarget = project === "" && !global;

  return (
    <Dialog open={visible} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>使用 {detail.key}</DialogTitle>
          <DialogDescription>
            把技能铺进项目目录或全局技能目录的 agent 位，并登记这次用法
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-1.5">
          <span className="text-xs font-medium">项目目录</span>
          <div className="flex items-center gap-1.5">
            <p className="min-w-0 flex-1 truncate rounded-md border border-border px-2 py-1.5 text-xs text-muted-foreground">
              {project === "" ? "尚未选择" : project}
            </p>
            <Button
              size="sm"
              variant="outline"
              onClick={() => void pickProject()}
            >
              选择…
            </Button>
            {project === "" ? null : (
              <Button size="sm" variant="ghost" onClick={() => setProject("")}>
                清除
              </Button>
            )}
          </div>
        </div>

        <label
          className={cn(
            "flex cursor-pointer items-center gap-2 text-xs",
            FOCUS_RING,
          )}
        >
          <input
            type="checkbox"
            checked={global}
            onChange={(event) => setGlobal(event.target.checked)}
            className="size-3.5 accent-primary"
          />
          <span className="font-medium">装到全局技能目录</span>
          <span className="text-muted-foreground">
            一台机器一份，与项目目标可并存
          </span>
        </label>

        <fieldset className="flex flex-col gap-1.5">
          <legend className="text-xs font-medium">Agent</legend>
          <div className="max-h-40 overflow-auto rounded-md border border-border p-1">
            {(agentList.data ?? []).map((agent) => (
              <label
                key={agent.key}
                className={cn(
                  "flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-xs hover:bg-accent",
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
                <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                  {/* 目录随勾选的目标走：选全局就看全局位，缺位直接点明，不必等后端报错 */}
                  {global
                    ? (agent.global_skills_dir ?? "无全局位")
                    : (agent.skills_dir ?? "无项目位")}
                </span>
              </label>
            ))}
          </div>
          {agentList.isError ? (
            <p className="flex items-center gap-2 text-xs text-destructive">
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
          <label htmlFor="use-version" className="text-xs font-medium">
            版本
          </label>
          <select
            id="use-version"
            value={version}
            onChange={(event) => setVersion(event.target.value)}
            className={cn(
              "h-8 rounded-md border border-border bg-background px-2 text-xs",
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

        <DialogFooter>
          <p className="mr-auto text-xs text-muted-foreground">
            {noTarget ? "至少选一个目标：项目目录或全局" : null}
          </p>
          <Button
            size="sm"
            disabled={noTarget || agents.length === 0 || use.isPending}
            onClick={() => use.mutate()}
          >
            {use.isPending ? "正在接入…" : "接入"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
