import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LibraryPage } from "@/app/library";
import { selectRows } from "@/app/library/parts";
import type { SkillSummary } from "@/lib/ipc";

// 替换 ipc 模块，固定一份清单与详情，不启动 Tauri
const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  showSkill: vi.fn(),
}));

vi.mock("@/lib/ipc", () => ({
  api: {
    listSkills: mocks.listSkills,
    showSkill: mocks.showSkill,
  },
  isGuiError: (value: unknown) =>
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value,
}));

const now = Math.floor(Date.now() / 1000);
const alpha: SkillSummary = {
  key: "alpha",
  latest_version: "1.0.0",
  archived: false,
  source: "owner/repo",
  description: "甲技能",
  tags: ["rust"],
  updated: now - 86_400,
};
const beta: SkillSummary = {
  key: "beta",
  latest_version: null,
  archived: true,
  source: null,
  description: null,
  tags: [],
  updated: null,
};

function detailFor(key: string, frontmatter: unknown) {
  return {
    key,
    latest_version: key === "alpha" ? "1.0.0" : null,
    archived: key !== "alpha",
    frontmatter,
    versions: key === "alpha" ? ["1.0.0"] : [],
    referenced_by: [],
    destination_count: key === "alpha" ? 1 : null,
    content: `# ${key}\n`,
  };
}

function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <LibraryPage />
    </QueryClientProvider>,
  );
}

describe("库页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listSkills.mockResolvedValue([alpha, beta]);
    mocks.showSkill.mockImplementation((key: string) =>
      Promise.resolve(
        key === "alpha"
          ? detailFor("alpha", {
              name: "alpha",
              description: "甲技能",
              version: "1.0.0",
              source: "owner/repo",
              tags: ["rust"],
            })
          : detailFor("beta", null),
      ),
    );
  });

  it("清单可见，选中行带着详情", async () => {
    renderPage();

    expect(await screen.findByRole("button", { name: /alpha/ })).toBeTruthy();
    // 描述同时出现在清单行与详情概览：限域断言详情一侧
    const detail = screen.getByRole("region", { name: "技能详情" });
    expect(await within(detail).findByText("甲技能")).toBeTruthy();
    expect(mocks.showSkill).toHaveBeenCalledWith("alpha");
  });

  it("搜索即筛，无结果时清除筛选复原", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const search = screen.getByLabelText("搜索技能");
    fireEvent.change(search, { target: { value: "beta" } });
    expect(screen.queryByRole("button", { name: /alpha/ })).toBeNull();
    expect(screen.getByRole("button", { name: /beta/ })).toBeTruthy();

    fireEvent.change(search, { target: { value: "查无此词" } });
    expect(await screen.findByText("没有匹配的技能")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "清除筛选" }));
    expect(screen.getByRole("button", { name: /alpha/ })).toBeTruthy();
  });

  it("归档分段切「已归档」后只剩归档项", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const segments = screen.getByRole("group", { name: "归档态" });
    fireEvent.click(within(segments).getByRole("button", { name: "已归档" }));
    expect(screen.queryByRole("button", { name: /alpha/ })).toBeNull();
    expect(screen.getByRole("button", { name: /beta/ })).toBeTruthy();
  });

  it("点标签 chip 后清单按标签收敛", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const list = screen.getByRole("region", { name: "技能列表" });
    fireEvent.click(within(list).getByRole("button", { name: "#rust" }));
    expect(screen.queryByRole("button", { name: /beta/ })).toBeNull();
    expect(screen.getByRole("button", { name: /alpha/ })).toBeTruthy();
  });

  it("frontmatter 缺失时详情顶部给黄条", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    fireEvent.click(screen.getByRole("button", { name: /beta/ }));
    expect(
      await screen.findByText(/SKILL.md 缺失或 frontmatter 无法解析/),
    ).toBeTruthy();
  });

  it("落点未知时统计卡显示破折号", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    fireEvent.click(screen.getByRole("button", { name: /beta/ }));
    const detail = screen.getByRole("region", { name: "技能详情" });
    const term = await within(detail).findByText("落点");
    expect(term.nextElementSibling?.textContent).toBe("—");
  });
});

describe("selectRows", () => {
  const rows: SkillSummary[] = [
    { ...alpha, key: "b-second", updated: now - 10 },
    { ...alpha, key: "a-first", updated: now - 900 },
    { ...beta, key: "c-archived", tags: ["rust", "office"] },
  ];
  const base = {
    query: "",
    archive: "all" as const,
    tags: [],
    sort: "updated" as const,
  };

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
});
