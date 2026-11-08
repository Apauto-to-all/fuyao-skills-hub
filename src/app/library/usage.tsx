import { Button } from "@/components/ui/button";
import type { SkillDetail, SkillUsage } from "@/lib/ipc";
import { useDetachFlow } from "../detach-flow";
import { EmptyState } from "./parts";

/** 版本说法：跟最新时如实写「跟随最新」，不冒充某个 tag */
function versionLabel(version: string | null): string {
  return version === null ? "跟随最新" : `v${version}`;
}

/** 一行记录的目标：项目记录写路径，全局记录写「全局」 */
function targetLabel(usage: SkillUsage): string {
  return usage.scope === "project" ? usage.project : "全局";
}

/**
 * 使用 tab：一条登记记录一行，按作用域分组（全局在前、项目在后）。
 * 目标、agent 名单与登记版本直接取自详情载荷；项目段的记录可原地解除，
 * 全局段的撤回走命令行。
 */
export function UsageTab({ detail }: { detail: SkillDetail }) {
  const detach = useDetachFlow();
  const global = detail.usage.filter((usage) => usage.scope === "global");
  const project = detail.usage.filter((usage) => usage.scope === "project");

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

  return (
    <div className="flex flex-col gap-5">
      {global.length > 0 ? (
        <UsageSection
          title="全局使用"
          caption="铺给全局技能目录，每个项目都看得见；撤回走 fsh unuse --global"
          rows={global}
        />
      ) : null}
      {project.length > 0 ? (
        <UsageSection
          title="项目使用"
          caption="只在这个项目里生效"
          rows={project}
          onDetach={(usage) =>
            detach.request({
              project: usage.project,
              skill: { key: detail.key, agents: usage.agents },
            })
          }
        />
      ) : null}
      {detach.dialog}
    </div>
  );
}

/** 行身份：作用域 + 目标 + 登记版本 + agent 名单，同一记录不会重复出行 */
function rowKey(usage: SkillUsage): string {
  const target = usage.scope === "project" ? usage.project : "";
  return `${usage.scope}:${target}:${usage.version ?? "latest"}:${usage.agents.join(",")}`;
}

/**
 * 一个作用域的分组：标题、作用域说明与逐记录的表格。
 * 行类型随分组而定——给了 onDetach 的分组才能拿到带项目路径的记录
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
  /** 给了才渲染行尾的操作列；全局作用域没有本地撤回入口 */
  onDetach?: (usage: S) => void;
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
              key={rowKey(usage)}
              className="border-b border-border/60 align-top"
            >
              <td
                className="max-w-60 truncate py-1.5 pr-3"
                title={usage.scope === "project" ? usage.project : undefined}
              >
                {targetLabel(usage)}
              </td>
              <td className="py-1.5 pr-3">{usage.agents.join("、")}</td>
              <td className="py-1.5 pr-3 tabular-nums">
                {versionLabel(usage.version)}
              </td>
              <td className="py-1.5 text-right">
                {onDetach ? (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => onDetach(usage)}
                  >
                    解除
                  </Button>
                ) : null}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}
