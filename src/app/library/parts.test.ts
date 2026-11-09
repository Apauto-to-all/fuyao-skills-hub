import { describe, expect, it } from "vitest";
import { selectRows } from "@/app/library/parts";
import type { SkillSummary } from "@/lib/ipc";
import { NOW, skillRow } from "@/test/fixtures";

// 纯规则直测：筛选与排序不碰渲染，也不经过查询
const rows: SkillSummary[] = [
  skillRow({ key: "b-second", updated: NOW - 10 }),
  skillRow({ key: "a-first", updated: NOW - 900 }),
  skillRow({
    key: "c-archived",
    archived: true,
    updated: null,
    tags: ["rust", "office"],
  }),
];
const base = {
  query: "",
  archive: "all" as const,
  tags: [],
  sort: "updated" as const,
};

describe("selectRows", () => {
  it("归档三段分别过滤活跃与归档", () => {
    expect(selectRows(rows, { ...base, archive: "archived" })).toHaveLength(1);
    expect(selectRows(rows, { ...base, archive: "active" })).toHaveLength(2);
  });

  it("多标签按 AND 收敛", () => {
    const hit = selectRows(rows, { ...base, tags: ["rust", "office"] });
    expect(hit.map((row) => row.key)).toEqual(["c-archived"]);
    expect(selectRows(rows, { ...base, tags: ["rust", "nope"] })).toHaveLength(
      0,
    );
  });

  it("按名称排序时键升序，最近更新时新在前", () => {
    expect(
      selectRows(rows, { ...base, sort: "name" }).map((row) => row.key),
    ).toEqual(["a-first", "b-second", "c-archived"]);
    expect(selectRows(rows, base).map((row) => row.key)).toEqual([
      "b-second",
      "a-first",
      "c-archived",
    ]);
  });

  it("关键词匹配键、描述与标签", () => {
    expect(
      selectRows(rows, { ...base, query: " office " }).map((row) => row.key),
    ).toEqual(["c-archived"]);
  });
});
