import { AlertTriangle } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api, type SkillDetail } from "@/lib/ipc";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { ConfirmDialog } from "../confirm-dialog";
import { Pill, StatCard, TagChip } from "./parts";
import { ReleaseDialog } from "./release-dialog";
import { UpgradeDialog } from "./upgrade-dialog";
import { UsageTab } from "./usage";
import { UseDialog } from "./use-dialog";
import { DiffSheet, VersionTimeline } from "./versions";

type Tab = "overview" | "versions" | "usage";

/** 详情栏：头部（键、来源、版本/归档标记、动作）+ 概览 / 版本 / 使用三分区 */
export function DetailPane({
  detail,
  selectedTags,
  onTagClick,
  onRemoved,
}: {
  detail: SkillDetail;
  selectedTags: string[];
  onTagClick: (tag: string) => void;
  onRemoved: () => void;
}) {
  const [tab, setTab] = useState<Tab>("overview");
  const [releaseOpen, setReleaseOpen] = useState(false);
  const [useOpen, setUseOpen] = useState(false);
  // 要切到的版本；非 null 即弹记录勾选框
  const [upgradeTo, setUpgradeTo] = useState<string | null>(null);
  const [diffPair, setDiffPair] = useState<{ from: string; to: string } | null>(
    null,
  );
  const [confirming, setConfirming] = useState<
    "archive" | "restore" | "remove" | null
  >(null);
  const frontmatter = detail.frontmatter;

  const archive = useHubMutation({
    mutationFn: () => api.archiveSkill(detail.key),
    success: "已归档",
    failure: "归档失败",
    onSuccess: () => setConfirming(null),
  });
  const restore = useHubMutation({
    mutationFn: () => api.restoreSkill(detail.key),
    success: "已恢复",
    failure: "恢复失败",
    onSuccess: () => setConfirming(null),
  });

  const remove = useHubMutation({
    mutationFn: () => api.removeSkill(detail.key),
    success: "已移除",
    failure: "移除失败",
    onSuccess: () => {
      setConfirming(null);
      onRemoved();
    },
  });

  const busy = archive.isPending || restore.isPending || remove.isPending;
  // 引用闸门看全部使用记录：项目与全局任一条存在都不能移除
  const referenced = detail.usage.length;

  return (
    <Tabs
      value={tab}
      onValueChange={(value) => setTab(value as Tab)}
      className="flex min-h-full flex-col gap-0"
    >
      <header className="border-b border-border px-6 pt-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-40 flex-1">
            <h2 className="truncate text-lg font-semibold" title={detail.key}>
              {detail.key}
            </h2>
            <p className="mt-0.5 truncate text-xs text-muted-foreground">
              {frontmatter?.source ?? "—"}
            </p>
          </div>
          <div className="flex shrink-0 flex-col items-end gap-2">
            <div className="flex items-center gap-1.5">
              {detail.latest_version ? (
                <Pill>v{detail.latest_version}</Pill>
              ) : null}
              {detail.archived ? <Pill tone="warning">已归档</Pill> : null}
            </div>
            <div className="flex items-center gap-1.5">
              {detail.archived ? (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setConfirming("restore")}
                >
                  恢复
                </Button>
              ) : (
                <>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => setUseOpen(true)}
                  >
                    使用…
                  </Button>
                  <Button size="sm" onClick={() => setReleaseOpen(true)}>
                    发版
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => setConfirming("archive")}
                  >
                    归档
                  </Button>
                </>
              )}
              <Button
                size="sm"
                variant="ghost"
                className="text-destructive"
                disabled={referenced > 0}
                title={referenced > 0 ? "仍被引用，先解除使用" : undefined}
                onClick={() => setConfirming("remove")}
              >
                移除
              </Button>
            </div>
          </div>
        </div>

        <TabsList
          variant="line"
          className="mt-3 h-auto w-full justify-start gap-4 rounded-none bg-transparent p-0"
        >
          {(
            [
              ["overview", "概览"],
              ["versions", "版本"],
              ["usage", "使用"],
            ] as const
          ).map(([value, label]) => (
            <TabsTrigger
              key={value}
              value={value}
              className="flex-none px-0 pb-2 text-sm text-muted-foreground after:bg-brand data-active:font-medium data-active:text-foreground"
            >
              {label}
            </TabsTrigger>
          ))}
        </TabsList>
      </header>

      <TabsContent value="overview" className="px-6 py-4">
        <Overview
          detail={detail}
          selectedTags={selectedTags}
          onTagClick={onTagClick}
        />
      </TabsContent>
      <TabsContent value="versions" className="px-6 py-4">
        <VersionTimeline
          timeline={detail.timeline}
          usage={detail.usage}
          onCompare={(from, to) => setDiffPair({ from, to })}
          onUpgrade={setUpgradeTo}
        />
      </TabsContent>
      <TabsContent value="usage" className="px-6 py-4">
        <UsageTab detail={detail} />
      </TabsContent>

      <UseDialog
        key={`use-${detail.key}`}
        detail={detail}
        open={useOpen}
        onOpenChange={setUseOpen}
      />
      <UpgradeDialog
        key={`upgrade-${detail.key}@${upgradeTo ?? ""}`}
        detail={detail}
        version={upgradeTo ?? ""}
        open={upgradeTo !== null}
        onOpenChange={(open) => !open && setUpgradeTo(null)}
      />
      <ReleaseDialog
        key={`${detail.key}@${detail.latest_version ?? ""}`}
        detail={detail}
        open={releaseOpen}
        onOpenChange={setReleaseOpen}
      />
      <DiffSheet
        skillKey={detail.key}
        pair={diffPair}
        onClose={() => setDiffPair(null)}
      />
      <ConfirmDialog
        open={confirming !== null}
        onOpenChange={(open) => !open && setConfirming(null)}
        title={
          confirming === "restore"
            ? "恢复技能"
            : confirming === "remove"
              ? "移除技能"
              : "归档技能"
        }
        description={
          confirming === "restore"
            ? `从最新版本重建 ${detail.key} 的编辑面；不产生新版本。`
            : confirming === "remove"
              ? `删除 ${detail.key}：${detail.timeline.length} 个版本 tag、编辑面与导出目录；在用记录 ${referenced} 处。此操作不可撤销。`
              : `删除 ${detail.key} 的编辑面与导出目录；版本 tag 与历史保留，可随时恢复。`
        }
        confirmLabel={
          confirming === "restore"
            ? "恢复"
            : confirming === "remove"
              ? "移除"
              : "归档"
        }
        destructive={confirming === "remove"}
        pending={busy}
        onConfirm={() => {
          // 框留到 mutation 落定再关，pending 才有机会显示
          if (confirming === "restore") restore.mutate();
          else if (confirming === "remove") remove.mutate();
          else archive.mutate();
        }}
      />
    </Tabs>
  );
}

function Overview({
  detail,
  selectedTags,
  onTagClick,
}: {
  detail: SkillDetail;
  selectedTags: string[];
  onTagClick: (tag: string) => void;
}) {
  const frontmatter = detail.frontmatter;
  // 使用记录按作用域分开计：项目几条、全局几条，都取自同一份 usage
  const projectUsages = detail.usage.filter(
    (usage) => usage.scope === "project",
  ).length;
  const globalUsages = detail.usage.filter(
    (usage) => usage.scope === "global",
  ).length;
  return (
    <div className="flex flex-col gap-4">
      {frontmatter === null ? (
        <div className="flex flex-col gap-1 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs">
          <p className="flex items-start gap-2">
            <AlertTriangle
              className="mt-0.5 size-3.5 shrink-0 text-warning"
              aria-hidden="true"
            />
            SKILL.md 缺失或 frontmatter 无法解析，以下信息按空值展示
          </p>
          {/* 原因自带文件名与字段名，解析成功或读不到 SKILL.md 时不出这一行 */}
          {detail.frontmatter_error ? (
            <p className="break-all pl-5.5 font-mono text-warning">
              {detail.frontmatter_error}
            </p>
          ) : null}
        </div>
      ) : null}

      <p className="text-sm">{frontmatter?.description ?? "没有描述"}</p>

      {frontmatter && frontmatter.tags.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {frontmatter.tags.map((tag) => (
            <TagChip
              key={tag}
              tag={tag}
              active={selectedTags.includes(tag)}
              onToggle={onTagClick}
            />
          ))}
        </div>
      ) : null}

      <dl className="grid grid-cols-2 gap-2 xl:grid-cols-4">
        <StatCard label="最新版本" value={detail.latest_version ?? "—"} />
        <StatCard label="版本数" value={String(detail.timeline.length)} />
        <StatCard label="项目在用" value={String(projectUsages)} />
        <StatCard label="全局在用" value={String(globalUsages)} />
        <StatCard
          label="落点"
          value={
            detail.destination_count === null
              ? "—"
              : String(detail.destination_count)
          }
        />
      </dl>

      <section className="flex flex-col gap-1.5">
        <h3 className="text-xs font-medium">SKILL.md</h3>
        <pre className="max-h-96 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs whitespace-pre-wrap">
          {detail.content ?? "读不到 SKILL.md 内容"}
        </pre>
      </section>
    </div>
  );
}
