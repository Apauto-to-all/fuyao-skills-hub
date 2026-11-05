import { AlertTriangle } from "lucide-react";
import { useState } from "react";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { SkillDetail } from "@/lib/ipc";
import { Pill, StatCard, TagChip } from "./parts";

type Tab = "overview" | "versions" | "usage";

/** 详情栏：头部（键、来源、版本/归档标记）+ 概览 / 版本 / 使用三分区 */
export function DetailPane({
  detail,
  selectedTags,
  onTagClick,
}: {
  detail: SkillDetail;
  selectedTags: string[];
  onTagClick: (tag: string) => void;
}) {
  const [tab, setTab] = useState<Tab>("overview");
  const frontmatter = detail.frontmatter;

  return (
    <Tabs
      value={tab}
      onValueChange={(value) => setTab(value as Tab)}
      className="flex min-h-full flex-col gap-0"
    >
      <header className="border-b border-border px-6 pt-4">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="truncate text-display font-semibold">
              {detail.key}
            </h2>
            <p className="mt-0.5 truncate text-ui-sm text-muted-foreground">
              {frontmatter?.source ?? "—"}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            {detail.latest_version ? (
              <Pill>v{detail.latest_version}</Pill>
            ) : null}
            {detail.archived ? <Pill tone="warning">已归档</Pill> : null}
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
              className="flex-none px-0 pb-2 text-ui text-muted-foreground after:bg-brand data-active:font-medium data-active:text-foreground"
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
        <p className="text-ui-sm text-muted-foreground">版本时间线正在开发中</p>
      </TabsContent>
      <TabsContent value="usage" className="px-6 py-4">
        <p className="text-ui-sm text-muted-foreground">使用记录正在开发中</p>
      </TabsContent>
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
  return (
    <div className="flex flex-col gap-4">
      {frontmatter === null ? (
        <p className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-ui-sm">
          <AlertTriangle
            className="mt-0.5 size-3.5 shrink-0 text-warning"
            aria-hidden="true"
          />
          SKILL.md 缺失或 frontmatter 无法解析，以下信息按空值展示
        </p>
      ) : null}

      <p className="text-ui">{frontmatter?.description ?? "没有描述"}</p>

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
        <StatCard label="版本数" value={String(detail.versions.length)} />
        <StatCard
          label="引用项目"
          value={String(detail.referenced_by.length)}
        />
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
        <h3 className="text-ui-sm font-medium">SKILL.md</h3>
        <pre className="max-h-96 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-ui-sm whitespace-pre-wrap">
          {detail.content ?? "读不到 SKILL.md 内容"}
        </pre>
      </section>
    </div>
  );
}
