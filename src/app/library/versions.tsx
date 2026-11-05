import { useQuery } from "@tanstack/react-query";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { formatDate } from "@/lib/format";
import { api, type VersionEntry } from "@/lib/ipc";
import { cn } from "@/lib/utils";
import { Centered, ErrorBlock } from "../feedback";

/** 版本 tab：竖向时间线，最新一版标「当前」，其余可对比上一版 */
export function VersionTimeline({
  timeline,
  onCompare,
}: {
  timeline: VersionEntry[];
  onCompare: (from: string, to: string) => void;
}) {
  if (timeline.length === 0) {
    return <p className="text-xs text-muted-foreground">还没有发过版</p>;
  }
  return (
    <ol className="flex flex-col">
      {timeline.map((entry, index) => {
        const current = index === 0;
        // 上一版是时间线上的下一项（更旧）；最旧一版没有可比对象
        const older = timeline[index + 1];
        return (
          <li
            key={entry.version}
            className="relative flex gap-3 pb-4 last:pb-0"
          >
            <span
              aria-hidden="true"
              className={cn(
                "mt-1 size-2 shrink-0 rounded-full",
                current ? "bg-brand" : "bg-border",
              )}
            />
            {index < timeline.length - 1 ? (
              <span
                aria-hidden="true"
                className="absolute top-3 left-[3px] h-full w-px bg-border"
              />
            ) : null}
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <span className="flex items-baseline gap-2">
                <span className="text-sm font-medium tabular-nums">
                  v{entry.version}
                </span>
                {current ? (
                  <span className="rounded border border-brand/40 bg-brand/10 px-1 text-xs text-brand">
                    当前
                  </span>
                ) : null}
                <span className="ml-auto shrink-0 text-xs tabular-nums text-muted-foreground">
                  {formatDate(entry.date)}
                </span>
              </span>
              <span className="flex items-center gap-2">
                <span className="sr-only">提交 </span>
                <code className="text-xs text-muted-foreground">
                  {entry.commit}
                </code>
                {older ? (
                  <button
                    type="button"
                    onClick={() => onCompare(older.version, entry.version)}
                    aria-label={`对比 v${older.version} 与 v${entry.version}`}
                    className="rounded text-xs text-brand hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                  >
                    对比上一版
                  </button>
                ) : null}
              </span>
            </div>
          </li>
        );
      })}
    </ol>
  );
}

/** 右侧 diff 面板：按文件折叠，等宽字体，增删着色 */
export function DiffSheet({
  skillKey,
  pair,
  onClose,
}: {
  skillKey: string;
  pair: { from: string; to: string } | null;
  onClose: () => void;
}) {
  const from = pair?.from ?? "";
  const to = pair?.to ?? "";
  const diff = useQuery({
    queryKey: ["diff", skillKey, from, to],
    // enabled 为 false 时不发请求，空串只是占位
    queryFn: () => api.diffVersions(skillKey, from, to),
    enabled: pair !== null,
  });

  return (
    <Sheet open={pair !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent side="right" className="w-[560px] sm:max-w-[560px]">
        <SheetHeader>
          <SheetTitle>
            {pair ? `对比 v${pair.from} → v${pair.to}` : "版本对比"}
          </SheetTitle>
          <SheetDescription>只显示该技能子树内的文件改动</SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
          {pair === null ? null : diff.isPending ? (
            <Centered>正在读取差异…</Centered>
          ) : diff.isError ? (
            <ErrorBlock error={diff.error} onRetry={() => diff.refetch()} />
          ) : diff.data.length === 0 ? (
            <p className="text-xs text-muted-foreground">两个版本没有差异</p>
          ) : (
            <div className="flex flex-col gap-2">
              {diff.data.map((file) => (
                <details
                  key={file.path}
                  className="rounded-md border border-border"
                >
                  <summary className="cursor-pointer px-3 py-2 text-xs font-medium">
                    {file.path}
                  </summary>
                  <pre className="overflow-x-auto border-t border-border p-3 font-mono text-xs">
                    {patchLines(file.patch).map(({ key, line }) => (
                      <span
                        key={key}
                        className={cn(
                          "block whitespace-pre",
                          line.startsWith("+") && "text-success",
                          line.startsWith("-") && "text-destructive",
                          line.startsWith("@@") && "text-muted-foreground",
                        )}
                      >
                        {line}
                      </span>
                    ))}
                  </pre>
                </details>
              ))}
            </div>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}

/** diff 行没有稳定标识：内容 + 同内容出现次数拼出稳定键 */
function patchLines(patch: string) {
  const seen = new Map<string, number>();
  return patch.split("\n").map((line) => {
    const count = (seen.get(line) ?? 0) + 1;
    seen.set(line, count);
    return { key: `${line}#${count}`, line };
  });
}
