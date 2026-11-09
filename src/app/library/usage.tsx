import { EmptyState } from "@/components/empty-state";
import { Button } from "@/components/ui/button";
import type { SkillDetail, SkillUsage } from "@/lib/ipc";
import {
  groupByScope,
  usageRowKey,
  usageTargetLabel,
  usageTargets,
  usageVersionLabel,
} from "@/lib/registry";
import { useDetachFlow } from "../detach-flow";

/**
 * 使用 tab：一条登记记录一行，按作用域分组（全局在前、项目在后）。
 * 目标、agent 名单与登记版本直接取自详情载荷；两段的记录都能就地解除该一条，
 * 多条记录一次撤由页头的「解除…」入口承担。
 */
export function UsageTab({ detail }: { detail: SkillDetail }) {
  const detach = useDetachFlow();
  const groups = groupByScope(detail.usage);

  if (detail.usage.length === 0) {
    return (
      <EmptyState
        title="还没有被任何项目或全局使用"
        hint={
          detail.archived
            ? "技能已归档；恢复后才可接进项目"
            : "点详情头的「使用…」，把它接进某个项目或全局"
        }
      />
    );
  }

  // 行内解除只针对一条记录：项目段与全局段走同一套单目标流程
  const detachOne = (usage: SkillUsage) =>
    detach.request({
      key: detail.key,
      agents: usage.agents,
      targets: usageTargets([usage]),
    });

  return (
    <div className="flex flex-col gap-5">
      {groups.global.length > 0 ? (
        <UsageSection
          title="全局使用"
          caption="铺给全局技能目录，每个项目都看得见"
          rows={groups.global}
          onDetach={detachOne}
        />
      ) : null}
      {groups.project.length > 0 ? (
        <UsageSection
          title="项目使用"
          caption="只在这个项目里生效"
          rows={groups.project}
          onDetach={detachOne}
        />
      ) : null}
      {detach.dialog}
    </div>
  );
}

/**
 * 一个作用域的分组：标题、作用域说明与逐记录的表格。
 * 行类型随分组而定——两段都给了 onDetach，各自只撤该一条记录
 */
function UsageSection<S extends SkillUsage>({
  title,
  caption,
  rows,
  onDetach,
}: {
  title: string;
  caption: string;
  rows: S[];
  /** 行尾操作列：只针对这一条记录的解除 */
  onDetach: (usage: S) => void;
}) {
  return (
    <section aria-label={title} className="flex flex-col gap-1.5">
      <header>
        <h3 className="text-xs font-medium">{title}</h3>
        <p className="text-xs text-muted-foreground">{caption}</p>
      </header>
      <table className="w-full border-collapse text-sm">
        <thead>
          <tr className="border-b border-border text-left text-xs text-muted-foreground">
            <th scope="col" className="py-1.5 pr-3 font-medium">
              目标
            </th>
            <th scope="col" className="py-1.5 pr-3 font-medium">
              Agent
            </th>
            <th scope="col" className="py-1.5 pr-3 font-medium">
              版本
            </th>
            <th scope="col" className="py-1.5 font-medium">
              <span className="sr-only">操作</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((usage) => (
            <tr
              key={usageRowKey(usage)}
              className="border-b border-border/60 align-top"
            >
              <td
                className="max-w-60 truncate py-1.5 pr-3"
                title={usage.scope === "project" ? usage.project : undefined}
              >
                {usageTargetLabel(usage)}
              </td>
              <td className="py-1.5 pr-3">{usage.agents.join("、")}</td>
              <td className="py-1.5 pr-3 tabular-nums">
                {usageVersionLabel(usage.version)}
              </td>
              <td className="py-1.5 text-right">
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => onDetach(usage)}
                >
                  解除
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
