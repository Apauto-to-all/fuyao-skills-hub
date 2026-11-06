import { useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { hubQueries } from "@/lib/hub-queries";
import type { SkillDetail } from "@/lib/ipc";
import { DestinationList, useDetachFlow } from "../detach-flow";
import { ErrorBlock } from "../feedback";
import { EmptyState } from "./parts";

/** 使用 tab：这个技能被哪些项目用着、落点是否还在，行尾可解除 */
export function UsageTab({ detail }: { detail: SkillDetail }) {
  const projects = useQuery(hubQueries.projects);
  const detach = useDetachFlow();

  if (projects.isPending) {
    return <p className="text-xs text-muted-foreground">正在读取项目…</p>;
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
      <table className="w-full border-collapse text-sm">
        <thead>
          <tr className="border-b border-border text-left text-xs text-muted-foreground">
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
                  <DestinationList destinations={skill.destinations} />
                </td>
                <td className="py-1.5 text-right">
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      detach.request({ project: project.path, key: skill.key })
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

      {detach.dialog}
    </>
  );
}
