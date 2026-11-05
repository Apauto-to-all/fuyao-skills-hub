import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { toast } from "@/components/ui/toast";
import { api, isGuiError, type SkillDetail } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "../confirm-dialog";
import { ErrorBlock } from "../feedback";
import { EmptyState } from "./parts";

type Target = { project: string; key: string };

/** 使用 tab：这个技能被哪些项目用着、落点是否还在，行尾可解除 */
export function UsageTab({ detail }: { detail: SkillDetail }) {
  const [pending, setPending] = useState<Target | null>(null);
  const queryClient = useQueryClient();
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: ["projects"] });
    void queryClient.invalidateQueries({ queryKey: ["agents"] });
    void queryClient.invalidateQueries({ queryKey: ["skill", detail.key] });
    void queryClient.invalidateQueries({ queryKey: ["skills"] });
  };

  const unuse = useMutation({
    scope: { id: "hub-write" },
    mutationFn: (target: Target) => api.detachSkill(target.key, target.project),
    onSuccess: () => {
      toast.add({ title: "已解除使用", type: "success" });
      setPending(null);
      refresh();
    },
    onError: (error) => {
      toast.add({
        title:
          isGuiError(error) && error.code === "lock_held"
            ? "另一个 fsh 进程正在写，稍后重试"
            : "解除失败",
        description: isGuiError(error) ? error.message : String(error),
        type: "error",
      });
    },
  });

  if (projects.isPending) {
    return <p className="text-ui-sm text-muted-foreground">正在读取项目…</p>;
  }
  if (projects.isError) {
    return (
      <ErrorBlock error={projects.error} onRetry={() => projects.refetch()} />
    );
  }

  const rows = projects.data.flatMap((project) =>
    project.skills
      .filter((skill) => skill.key === detail.key)
      .map((skill) => ({ project, skill })),
  );

  if (rows.length === 0) {
    return (
      <EmptyState
        title="还没有项目在用这个技能"
        hint={
          detail.archived
            ? "技能已归档；恢复后才可接进项目"
            : "点详情头的「使用…」，把它接进某个项目"
        }
      />
    );
  }

  return (
    <>
      <table className="w-full border-collapse text-ui">
        <thead>
          <tr className="border-b border-border text-left text-meta text-muted-foreground">
            <th scope="col" className="py-1.5 pr-3 font-medium">
              项目
            </th>
            <th scope="col" className="py-1.5 pr-3 font-medium">
              版本
            </th>
            <th scope="col" className="py-1.5 pr-3 font-medium">
              Agent
            </th>
            <th scope="col" className="py-1.5 pr-3 font-medium">
              落点
            </th>
            <th scope="col" className="py-1.5 font-medium">
              <span className="sr-only">操作</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map(({ project, skill }) => {
            return (
              <tr
                key={`${project.path}:${skill.key}`}
                className="border-b border-border/60 align-top"
              >
                <td
                  className="max-w-60 truncate py-1.5 pr-3"
                  title={project.path}
                >
                  {project.path}
                </td>
                <td className="py-1.5 pr-3 tabular-nums">
                  {skill.version === null ? "跟随最新" : `v${skill.version}`}
                </td>
                <td className="py-1.5 pr-3">{skill.agents.join("、")}</td>
                <td className="max-w-64 py-1.5 pr-3">
                  <ul className="flex flex-col gap-0.5">
                    {skill.destinations.map((dest) => (
                      <li
                        key={dest.path}
                        className="flex items-center gap-1.5 text-meta"
                      >
                        <span
                          aria-hidden="true"
                          className={cn(
                            "size-1.5 shrink-0 rounded-full",
                            dest.exists ? "bg-success" : "bg-destructive",
                          )}
                        />
                        <span className="truncate" title={dest.path}>
                          {dest.path}
                        </span>
                        {dest.exists ? null : (
                          <span className="shrink-0 text-destructive">
                            缺失
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                </td>
                <td className="py-1.5 text-right">
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      setPending({ project: project.path, key: skill.key })
                    }
                  >
                    解除
                  </Button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

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
    </>
  );
}
