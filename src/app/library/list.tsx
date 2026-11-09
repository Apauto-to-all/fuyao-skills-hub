import { ArrowUpDown, Search } from "lucide-react";
import { EmptyState } from "@/components/empty-state";
import { Button } from "@/components/ui/button";
import { FOCUS_RING } from "@/lib/focus-ring";
import { relativeTime } from "@/lib/format";
import {
  api,
  type GuiError,
  type SearchHit,
  type SkillSummary,
} from "@/lib/ipc";
import { pickDirectory } from "@/lib/pick-directory";
import { splitBySpans } from "@/lib/split-by-spans";
import { useHubMutation } from "@/lib/use-hub-mutation";
import { useJumpScroll } from "@/lib/use-jump-scroll";
import { cn } from "@/lib/utils";
import {
  ARCHIVE_LABELS,
  type ArchiveFilter,
  type SortMode,
  TagChip,
} from "./parts";

type Props = {
  rows: SkillSummary[];
  total: number;
  tags: string[];
  query: string;
  /** 检索请求在途：清单先按旧数据渲染，空态文案换成检索中 */
  searching: boolean;
  /** 检索失败：过滤不生效，错误以行内提示露出 */
  searchError: GuiError | null;
  /** 键到命中行的映射；查询框空时省略 */
  hitsByKey?: Map<string, SearchHit[]>;
  archive: ArchiveFilter;
  selectedTags: string[];
  sort: SortMode;
  activeKey: string | null;
  /** 定位跳转的序号：同一个 key 再跳一次也要重新滚到该行 */
  activeSeq: number;
  /** 定位跳转的目标键：只有它成为选中行才居中，点击行仍按最小滚动 */
  jumpKey: string | null;
  onQuery: (value: string) => void;
  onArchive: (value: ArchiveFilter) => void;
  onSort: () => void;
  onToggleTag: (tag: string) => void;
  onSelect: (key: string) => void;
  onClear: () => void;
};

/** 列表栏：筛选条 + 清单 + 两种空态 */
export function ListPane({
  rows,
  total,
  tags,
  query,
  searching,
  searchError,
  hitsByKey,
  archive,
  selectedTags,
  sort,
  activeKey,
  activeSeq,
  jumpKey,
  onQuery,
  onArchive,
  onSort,
  onToggleTag,
  onSelect,
  onClear,
}: Props) {
  const activeRow = useJumpScroll<HTMLLIElement>({
    activeKey,
    seq: activeSeq,
    jumpKey,
  });

  return (
    <section
      aria-label="技能列表"
      className="flex w-[380px] shrink-0 flex-col border-r border-border"
    >
      <div className="flex flex-col gap-2 border-b border-border p-3">
        <div className="relative">
          <Search
            className="pointer-events-none absolute top-2 left-2 size-3.5 text-muted-foreground"
            aria-hidden="true"
          />
          <input
            type="search"
            value={query}
            onChange={(event) => onQuery(event.target.value)}
            placeholder="全文检索技能内容…"
            aria-label="搜索技能"
            className={cn(
              "h-8 w-full rounded-md border border-input bg-background pr-2 pl-7 text-sm outline-none",
              FOCUS_RING,
            )}
          />
        </div>

        <div className="flex items-center justify-between gap-2">
          <fieldset className="flex rounded-md border border-border p-0.5">
            <legend className="sr-only">归档态</legend>
            {ARCHIVE_LABELS.map(({ value, label }) => (
              <button
                key={value}
                type="button"
                aria-pressed={archive === value}
                onClick={() => onArchive(value)}
                className={cn(
                  "h-6 rounded px-2 text-xs transition-colors duration-100",
                  FOCUS_RING,
                  archive === value
                    ? "bg-accent font-medium text-foreground"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                {label}
              </button>
            ))}
          </fieldset>
          <button
            type="button"
            onClick={onSort}
            aria-label={`切换排序方式（当前：${sort === "updated" ? "最近更新" : "名称"}）`}
            className={cn(
              "flex shrink-0 items-center gap-1 rounded text-xs text-muted-foreground hover:text-foreground",
              FOCUS_RING,
            )}
          >
            <ArrowUpDown className="size-3" aria-hidden="true" />
            {sort === "updated" ? "最近更新" : "名称"}
          </button>
        </div>

        {searchError ? (
          <p role="status" className="text-xs text-warning">
            {searchError.message}（本次不过滤）
          </p>
        ) : null}

        {tags.length > 0 ? (
          <fieldset className="flex flex-wrap gap-1">
            <legend className="sr-only">标签筛选</legend>
            {tags.map((tag) => (
              <TagChip
                key={tag}
                tag={tag}
                active={selectedTags.includes(tag)}
                onToggle={onToggleTag}
              />
            ))}
          </fieldset>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {total === 0 ? <EmptyStack /> : null}
        {total > 0 && rows.length === 0 ? (
          <EmptyState
            title={searching ? "正在检索…" : "没有匹配的技能"}
            hint={searching ? "内容检索进行中" : "换个关键词，或放宽筛选条件"}
            action={
              <Button variant="outline" size="sm" onClick={onClear}>
                清除筛选
              </Button>
            }
          />
        ) : null}
        <ul>
          {rows.map((row) => (
            <li
              key={row.key}
              ref={row.key === activeKey ? activeRow : undefined}
            >
              <button
                type="button"
                onClick={() => onSelect(row.key)}
                aria-current={row.key === activeKey ? "true" : undefined}
                className={cn(
                  "flex w-full flex-col gap-0.5 border-l-2 border-transparent px-3 py-2 text-left transition-colors duration-100 hover:bg-accent/50",
                  FOCUS_RING,
                  row.key === activeKey && "border-brand bg-muted/60",
                )}
              >
                <span className="flex items-baseline gap-2">
                  <span
                    className={cn(
                      "truncate text-sm font-medium",
                      row.archived && "text-muted-foreground",
                    )}
                  >
                    {row.key}
                  </span>
                  <span className="ml-auto flex shrink-0 items-baseline gap-1.5 text-xs tabular-nums text-muted-foreground">
                    {row.archived ? (
                      <span className="rounded border border-border px-1">
                        已归档
                      </span>
                    ) : null}
                    {row.latest_version ? `v${row.latest_version}` : "无版本"}
                    {row.updated === null ? null : (
                      <span>{relativeTime(row.updated)}</span>
                    )}
                  </span>
                </span>
                <span className="flex items-baseline gap-1.5 text-xs text-muted-foreground">
                  {row.tags.map((tag) => (
                    <span key={tag} className="shrink-0 text-brand">
                      #{tag}
                    </span>
                  ))}
                  <span className="truncate">
                    {row.description ?? "无描述"}
                  </span>
                </span>
                <HitSnippets hits={hitsByKey?.get(row.key)} />
              </button>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}

/** 单条命中行渲染：定位（路径:行号）+ 上下文行与命中行（spans 高亮） */
function HitSnippet({ hit }: { hit: SearchHit }) {
  const context = hit.context;
  return (
    <span className="flex flex-col gap-0.5 rounded border border-border bg-muted/30 px-2 py-1.5 font-mono text-[11px] leading-5">
      <span className="text-muted-foreground">
        {hit.path}:{hit.line}
      </span>
      {context?.before.map((line) => (
        <span
          key={`b${line.line}`}
          className="truncate text-muted-foreground/70"
        >
          {line.text || " "}
        </span>
      ))}
      <span className="truncate">
        {splitBySpans(hit.text, hit.spans).map((seg) =>
          seg.hit ? (
            <mark
              key={seg.start}
              className="rounded-sm bg-warning/30 text-foreground"
            >
              {seg.text}
            </mark>
          ) : (
            <span key={seg.start}>{seg.text}</span>
          ),
        )}
      </span>
      {context?.after.map((line) => (
        <span
          key={`a${line.line}`}
          className="truncate text-muted-foreground/70"
        >
          {line.text || " "}
        </span>
      ))}
    </span>
  );
}

/** 键的命中片段组：每键最多展示前三条，多余以「另有 N 处」收口 */
function HitSnippets({ hits }: { hits?: SearchHit[] }) {
  if (!hits?.length) return null;
  const shown = hits.slice(0, 3);
  const rest = hits.length - shown.length;
  return (
    <span className="flex flex-col gap-1">
      {shown.map((hit) => (
        <HitSnippet key={`${hit.path}:${hit.line}`} hit={hit} />
      ))}
      {rest > 0 ? (
        <span className="text-[11px] text-muted-foreground">
          另有 {rest} 处命中
        </span>
      ) : null}
    </span>
  );
}

/** 空库引导：原生目录选择器把本地 skill 目录入库 */
function EmptyStack() {
  const add = useHubMutation({
    mutationFn: (source: string) => api.addSkill(source),
    success: (report) => `已入库 ${report.key} v${report.version}`,
    failure: "入库失败",
  });

  const pick = async () => {
    const picked = await pickDirectory("选择 skill 目录（含 SKILL.md）");
    if (picked !== null) add.mutate(picked);
  };

  return (
    <EmptyState
      title="库里还没有技能"
      hint="选一个含 SKILL.md 的本地目录，入库后会立即出现在清单里"
      action={
        <Button size="sm" disabled={add.isPending} onClick={() => void pick()}>
          {add.isPending ? "正在入库…" : "从目录入库…"}
        </Button>
      }
    />
  );
}
