import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { api } from "@/lib/ipc";
import { Centered, ErrorBlock } from "../feedback";
import { DetailPane } from "./detail";
import { ListPane } from "./list";
import { type ArchiveFilter, type SortMode, selectRows } from "./parts";

/** 库页：左侧清单 + 右侧详情，筛选状态只存在页面内 */
export function LibraryPage({
  focusKey,
}: {
  focusKey?: { key: string; seq: number } | null;
}) {
  const [query, setQuery] = useState("");
  const [archive, setArchive] = useState<ArchiveFilter>("all");
  const [tags, setTags] = useState<string[]>([]);
  const [sort, setSort] = useState<SortMode>("updated");
  const [selected, setSelected] = useState<string | null>(
    focusKey?.key ?? null,
  );

  // 命令面板跳技能：清掉筛选再选中，否则目标会被当前筛选静默滤掉
  useEffect(() => {
    if (!focusKey) return;
    setQuery("");
    setArchive("all");
    setTags([]);
    setSelected(focusKey.key);
  }, [focusKey]);

  const skills = useQuery({ queryKey: ["skills"], queryFn: api.listSkills });
  const allTags = useMemo(
    () => [...new Set((skills.data ?? []).flatMap((row) => row.tags))].sort(),
    [skills.data],
  );
  const visible = useMemo(
    () => selectRows(skills.data ?? [], { query, archive, tags, sort }),
    [skills.data, query, archive, tags, sort],
  );
  // 选中项被筛掉时回落到首行：详情永远对着清单里看得见的一项
  const active =
    visible.find((row) => row.key === selected) ?? visible[0] ?? null;

  const detail = useQuery({
    queryKey: ["skill", active?.key],
    // 查询键就是选中键：enabled 已挡住空选，取键不依赖闭包里的旧值
    queryFn: ({ queryKey }) => api.showSkill(String(queryKey[1])),
    enabled: active !== null,
  });

  const toggleTag = (tag: string) =>
    setTags((prev) =>
      prev.includes(tag) ? prev.filter((item) => item !== tag) : [...prev, tag],
    );

  const clearFilters = () => {
    setQuery("");
    setArchive("all");
    setTags([]);
  };

  if (skills.isPending) return <Centered>正在读取技能库…</Centered>;
  if (skills.isError)
    return <ErrorBlock error={skills.error} onRetry={() => skills.refetch()} />;

  return (
    <div className="flex h-full min-h-0">
      <ListPane
        rows={visible}
        total={skills.data.length}
        tags={allTags}
        query={query}
        archive={archive}
        selectedTags={tags}
        sort={sort}
        activeKey={active?.key ?? null}
        onQuery={setQuery}
        onArchive={setArchive}
        onSort={() => setSort(sort === "updated" ? "name" : "updated")}
        onToggleTag={toggleTag}
        onSelect={setSelected}
        onClear={clearFilters}
      />

      <section aria-label="技能详情" className="min-w-0 flex-1 overflow-y-auto">
        {active === null ? null : detail.isPending ? (
          <Centered>正在读取详情…</Centered>
        ) : detail.isError ? (
          <ErrorBlock error={detail.error} onRetry={() => detail.refetch()} />
        ) : detail.data ? (
          <DetailPane
            detail={detail.data}
            selectedTags={tags}
            onTagClick={toggleTag}
            onRemoved={() => setSelected(null)}
          />
        ) : null}
      </section>
    </div>
  );
}
