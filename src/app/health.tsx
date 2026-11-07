import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, CircleAlert, XCircle } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { formatBytes } from "@/lib/format";
import { hubQueries } from "@/lib/hub-queries";
import {
  api,
  type CheckStatus,
  type DoctorCheck,
  type GcAction,
} from "@/lib/ipc";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "./confirm-dialog";
import { Centered, ErrorBlock } from "./feedback";

/** 检查项稳定标识 → 人读名；未收录的新项直接显示原标识 */
const CHECK_LABELS: Record<string, string> = {
  hub_writable: "hub 根可写",
  cloud_path: "非云同步路径",
  link_capability: "链接能力",
  exported_missing: "导出目录完整",
  dead_projects: "项目记录有效",
  reclaimable: "可回收空间",
};

const STATUS_META: Record<
  CheckStatus,
  { icon: typeof CheckCircle2; tone: string; label: string }
> = {
  fail: { icon: XCircle, tone: "text-destructive", label: "失败" },
  warn: { icon: CircleAlert, tone: "text-warning", label: "警告" },
  ok: { icon: CheckCircle2, tone: "text-success", label: "通过" },
};

/** 回收动作的人读标签；大小列只有带字节的动作才有值 */
const ACTION_LABELS: Record<GcAction["action"], string> = {
  cleaned: "清理导出",
  orphan_cleaned: "清理孤儿目录",
  dirty_skipped: "跳过改动",
  dead_record_removed: "移除死记录",
  grace_kept: "宽限保留",
};

/** 这三类动作会改动磁盘或登记簿，其余只是提示 */
const isActionable = (item: GcAction) =>
  item.action === "cleaned" ||
  item.action === "orphan_cleaned" ||
  item.action === "dead_record_removed";

function CheckRow({ check }: { check: DoctorCheck }) {
  const meta = STATUS_META[check.status];
  const Icon = meta.icon;
  return (
    <li className="flex items-start gap-2 border-b border-border/60 py-1.5 last:border-b-0">
      <Icon
        className={cn("mt-0.5 size-3.5 shrink-0", meta.tone)}
        strokeWidth={1.75}
        aria-hidden="true"
      />
      <div className="min-w-0">
        <p className="text-sm">{CHECK_LABELS[check.name] ?? check.name}</p>
        <p
          className="truncate text-xs text-muted-foreground"
          title={check.detail}
        >
          {check.detail}
        </p>
      </div>
      <span className={cn("ml-auto shrink-0 text-xs", meta.tone)}>
        {meta.label}
      </span>
    </li>
  );
}

/** 健康页：自检只读三段分组，回收先预演后执行；两路查询互不阻塞 */
export function HealthPage() {
  const [confirmOpen, setConfirmOpen] = useState(false);
  // 执行后返回的报告直接顶掉预演表，避免再等一次预演
  const [executed, setExecuted] = useState<{
    items: GcAction[];
    freed_bytes: number;
  } | null>(null);

  const doctor = useQuery(hubQueries.doctor);
  const preview = useQuery(hubQueries.gcPreview);
  const run = useHubMutation({
    mutationFn: () => api.gc(false),
    success: (report) => `回收完成，释放 ${formatBytes(report.freed_bytes)}`,
    failure: "回收失败",
    onSuccess: (report) => {
      setExecuted({ items: report.items, freed_bytes: report.freed_bytes });
      setConfirmOpen(false);
    },
  });

  if (doctor.isPending || preview.isPending) {
    return <Centered>正在检查…</Centered>;
  }
  if (doctor.isError || preview.isError) {
    return (
      <ErrorBlock
        error={doctor.error ?? preview.error}
        onRetry={() => {
          void doctor.refetch();
          void preview.refetch();
        }}
      />
    );
  }

  const checks = doctor.data.checks;
  const fails = checks.filter((check) => check.status === "fail");
  const warns = checks.filter((check) => check.status === "warn");
  const passes = checks.filter((check) => check.status === "ok");

  const items = executed?.items ?? preview.data.items;
  const actionable = items.filter(isActionable);
  const bytes = executed?.freed_bytes ?? preview.data.freed_bytes;

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-6 px-6 py-4">
      <section aria-labelledby="doctor-heading">
        <div className="flex items-baseline justify-between">
          <h2 id="doctor-heading" className="text-sm font-semibold">
            自检
          </h2>
          <p className="text-xs text-muted-foreground">
            失败 {fails.length} 项 · 警告 {warns.length} 项
          </p>
        </div>
        {fails.length > 0 ? (
          <ul className="mt-1.5">
            {fails.map((check) => (
              <CheckRow key={check.name} check={check} />
            ))}
          </ul>
        ) : null}
        {warns.length > 0 ? (
          <ul className="mt-1.5">
            {warns.map((check) => (
              <CheckRow key={check.name} check={check} />
            ))}
          </ul>
        ) : null}
        {passes.length > 0 ? (
          <details className="mt-1.5">
            <summary className="cursor-pointer text-xs text-muted-foreground">
              通过 {passes.length} 项
            </summary>
            <ul className="mt-1">
              {passes.map((check) => (
                <CheckRow key={check.name} check={check} />
              ))}
            </ul>
          </details>
        ) : null}
      </section>

      <section aria-labelledby="gc-heading">
        <div className="flex items-center justify-between">
          <h2 id="gc-heading" className="text-sm font-semibold">
            {executed ? "已回收" : "可回收"} {formatBytes(bytes)}
          </h2>
          {actionable.length > 0 && !executed ? (
            <Button size="sm" onClick={() => setConfirmOpen(true)}>
              执行回收
            </Button>
          ) : null}
        </div>
        {items.length === 0 ? (
          <p className="mt-2 text-xs text-muted-foreground">没有可回收的内容</p>
        ) : (
          <table className="mt-2 w-full table-fixed border-collapse text-sm">
            <colgroup>
              <col className="w-52" />
              <col />
              <col className="w-20" />
            </colgroup>
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted-foreground">
                <th scope="col" className="py-1.5 pr-3 font-medium">
                  动作
                </th>
                <th scope="col" className="py-1.5 pr-3 font-medium">
                  对象
                </th>
                <th scope="col" className="py-1.5 text-right font-medium">
                  大小
                </th>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => {
                const dirty = item.action === "dirty_skipped";
                return (
                  <tr
                    key={`${item.action}-${item.path}`}
                    className="border-b border-border/60"
                  >
                    <td className="py-1.5 pr-3">
                      <span className={cn(dirty && "text-warning")}>
                        {ACTION_LABELS[item.action]}
                      </span>
                      {dirty ? (
                        <span className="ml-1.5 text-xs text-warning">
                          内容被改动，已跳过
                        </span>
                      ) : null}
                    </td>
                    <td
                      className="truncate py-1.5 pr-3 font-mono text-xs text-muted-foreground"
                      title={item.path}
                    >
                      <span className="block truncate">{item.path}</span>
                    </td>
                    <td className="py-1.5 text-right text-xs tabular-nums">
                      {"bytes" in item ? formatBytes(item.bytes) : "—"}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        <ConfirmDialog
          open={confirmOpen}
          onOpenChange={setConfirmOpen}
          title="执行回收"
          description={`将释放 ${formatBytes(preview.data.freed_bytes)}：删除无引用的导出目录与失效的项目记录。版本 tag 与提交历史不受影响。`}
          confirmLabel="执行回收"
          pending={run.isPending}
          onConfirm={() => run.mutate()}
        />
      </section>
    </div>
  );
}
