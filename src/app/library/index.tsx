import { useQuery } from "@tanstack/react-query";
import { useLayoutEffect, useMemo, useState } from "react";
import { hubQueries } from "@/lib/hub-queries";
import { type GuiError, isGuiError, type SearchHit } from "@/lib/ipc";
import type { FocusTarget } from "@/lib/use-jump-scroll";
import { Centered, ErrorBlock } from "../feedback";
import { DetailPane } from "./detail";
import { ListPane } from "./list";
import { type ArchiveFilter, type SortMode, selectRows } from "./parts";

/** 库页：左侧清单 + 右侧详情，筛选状态只存在页面内 */
export function LibraryPage({ focusKey }: { focusKey?: FocusTarget | null }) {
  const [query, setQuery] = useState("");
  const [archive, setArchive] = useState<ArchiveFilter>("all");
  const [tags, setTags] = useState<string[]>([]);
  const [sort, setSort] = useState<SortMode>("updated");
  const [selected, setSelected] = useState<string | null>(
    focusKey?.key ?? null,
  );

  // 命令面板跳技能：清掉筛选再选中，否则目标会被当前筛选静默滤掉
  useLayoutEffect(() => {
    if (!focusKey) return;
    setQuery("");
    setArchive("all");
    setTags([]);
    setSelected(focusKey.key);
  }, [focusKey]);

  const skills = useQuery(hubQueries.skills);
  // 内容检索：查询词有值才发；命中按 key 聚合成键集合（任一文件命中即保留该键）
  const trimmed = query.trim();
  const search = useQuery(hubQueries.search(trimmed));
  const matchedKeys = useMemo(
    () =>
      trimmed === "" || search.data === undefined
        ? null
        : new Set(
            search.data.hits
              .filter((hit) => hit.key !== undefined)
              .map((hit) => hit.key as string),
          ),
    [trimmed, search.data],
  );
  // 键集合到命中行的映射：清单行下方渲染片段用
  const hitsByKey = useMemo(() => {
    const map = new Map<string, SearchHit[]>();
    for (const hit of search.data?.hits ?? []) {
      if (hit.key === undefined) continue;
      const list = map.get(hit.key) ?? [];
      list.push(hit);
      map.set(hit.key, list);
    }
    return map;
  }, [search.data]);
  const allTags = useMemo(
    () => [...new Set((skills.data ?? []).flatMap((row) => row.tags))].sort(),
    [skills.data],
  );
  const visible = useMemo(
    () => selectRows(skills.data ?? [], { matchedKeys, archive, tags, sort }),
    [skills.data, matchedKeys, archive, tags, sort],
  );
  // 选中项被筛掉时回落到首行：详情永远对着清单里看得见的一项
  const active =
    visible.find((row) => row.key === selected) ?? visible[0] ?? null;

  const detail = useQuery({
    // 查询键就是选中键：enabled 已挡住空选，键不依赖闭包里的旧值
    ...hubQueries.skill(active?.key ?? ""),
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
  // 检索失败只废掉这一次过滤，不整页报错：清单照旧（不过滤）并给出错误条
  const searchError =
    trimmed !== "" && search.isError && isGuiError(search.error)
      ? (search.error as GuiError)
      : null;

  return (
    <div className="flex h-full min-h-0">
      <ListPane
        rows={visible}
        total={skills.data.length}
        tags={allTags}
        query={query}
        searching={search.isPending && trimmed !== ""}
        searchError={searchError}
        hitsByKey={trimmed === "" ? undefined : hitsByKey}
        archive={archive}
        selectedTags={tags}
        sort={sort}
        activeKey={active?.key ?? null}
        activeSeq={focusKey?.seq ?? 0}
        jumpKey={focusKey?.key ?? null}
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
