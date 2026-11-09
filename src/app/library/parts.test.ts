import { describe, expect, it } from "vitest";
import { selectRows } from "@/app/library/parts";
import type { SkillSummary } from "@/lib/ipc";
import { splitBySpans } from "@/lib/split-by-spans";
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
  matchedKeys: null as Set<string> | null,
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

  it("键集合过滤：集合内的键保留，集合外剔除；null 不过滤", () => {
    const keys = new Set(["a-first"]);
    expect(
      selectRows(rows, { ...base, matchedKeys: keys }).map((row) => row.key),
    ).toEqual(["a-first"]);
    // 描述含 office 但键不在集合内：内容检索语义下不保留
    expect(
      selectRows(rows, {
        ...base,
        matchedKeys: new Set(["b-second"]),
      }).map((row) => row.key),
    ).toEqual(["b-second"]);
    expect(selectRows(rows, { ...base, matchedKeys: new Set() }).length).toBe(
      0,
    );
  });

  it("键集合与归档、标签过滤叠加（AND）", () => {
    const keys = new Set(["b-second", "c-archived"]);
    expect(
      selectRows(rows, { ...base, matchedKeys: keys, archive: "archived" }).map(
        (row) => row.key,
      ),
    ).toEqual(["c-archived"]);
  });
});

describe("splitBySpans", () => {
  it("按字符偏移切成普通段与命中段", () => {
    expect(splitBySpans("beta 中文针 needle 中文", [[9, 15]])).toEqual([
      { start: 0, text: "beta 中文针 ", hit: false },
      { start: 9, text: "needle", hit: true },
      { start: 15, text: " 中文", hit: false },
    ]);
  });

  it("无 spans 整行普通段；多段命中逐段切分", () => {
    expect(splitBySpans("plain", [])).toEqual([
      { start: 0, text: "plain", hit: false },
    ]);
    expect(
      splitBySpans("aXbXc", [
        [1, 2],
        [3, 4],
      ]),
    ).toEqual([
      { start: 0, text: "a", hit: false },
      { start: 1, text: "X", hit: true },
      { start: 2, text: "b", hit: false },
      { start: 3, text: "X", hit: true },
      { start: 4, text: "c", hit: false },
    ]);
  });

  it("越界 span 按行尾裁剪", () => {
    expect(splitBySpans("ab", [[1, 99]])).toEqual([
      { start: 0, text: "a", hit: false },
      { start: 1, text: "b", hit: true },
    ]);
  });
});
