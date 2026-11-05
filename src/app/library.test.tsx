import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LibraryPage } from "@/app/library";
import { selectRows } from "@/app/library/parts";
import type { SkillSummary } from "@/lib/ipc";

// 替换 ipc 模块，固定一份清单与详情，不启动 Tauri
const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  showSkill: vi.fn(),
  diffVersions: vi.fn(),
  releaseSkill: vi.fn(),
  archiveSkill: vi.fn(),
  restoreSkill: vi.fn(),
  projects: vi.fn(),
  agents: vi.fn(),
  attachSkill: vi.fn(),
  detachSkill: vi.fn(),
  addSkill: vi.fn(),
}));

const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

vi.mock("@/lib/ipc", () => ({
  api: {
    listSkills: mocks.listSkills,
    showSkill: mocks.showSkill,
    diffVersions: mocks.diffVersions,
    releaseSkill: mocks.releaseSkill,
    archiveSkill: mocks.archiveSkill,
    restoreSkill: mocks.restoreSkill,
    projects: mocks.projects,
    agents: mocks.agents,
    attachSkill: mocks.attachSkill,
    detachSkill: mocks.detachSkill,
    addSkill: mocks.addSkill,
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
    latest_version: key === "alpha" ? "1.1.0" : null,
    archived: key !== "alpha",
    frontmatter,
    timeline:
      key === "alpha"
        ? [
            { version: "1.1.0", commit: "9f3a1c2", date: now - 86_400 },
            { version: "1.0.0", commit: "7b21e04", date: now - 3 * 86_400 },
          ]
        : [],
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
              version: "1.1.0",
              source: "owner/repo",
              tags: ["rust"],
            })
          : detailFor("beta", null),
      ),
    );
    mocks.diffVersions.mockResolvedValue([
      { path: "SKILL.md", patch: "@@ -1,2 +1,2 @@\n-旧描述\n+甲技能" },
    ]);
    mocks.releaseSkill.mockImplementation((_key: string, version: string) =>
      Promise.resolve(version),
    );
    mocks.archiveSkill.mockResolvedValue(undefined);
    mocks.restoreSkill.mockResolvedValue(undefined);
    mocks.projects.mockResolvedValue([]);
    mocks.agents.mockResolvedValue([]);
    mocks.detachSkill.mockResolvedValue({
      key: "alpha",
      project: "/p/demo",
      removed: [],
    });
  });

  it("空库点「从目录入库」调用 add_skill", async () => {
    mocks.listSkills.mockResolvedValue([]);
    dialog.open.mockResolvedValue("/tmp/demo");
    mocks.addSkill.mockResolvedValue({ key: "demo", version: "0.1.0" });
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "从目录入库…" }));
    await waitFor(() =>
      expect(mocks.addSkill).toHaveBeenCalledWith("/tmp/demo"),
    );
  });

  it("使用表单选目录与 agent 后提交", async () => {
    mocks.agents.mockResolvedValue([
      {
        key: "dsh",
        display_name: "DeepSeek Harness",
        skills_dir: ".dsh/skills",
        source: "built_in",
        usages: [],
      },
    ]);
    dialog.open.mockResolvedValue("D:/work/alpha");
    mocks.attachSkill.mockResolvedValue({
      key: "alpha",
      version: null,
      project: "D:/work/alpha",
      agents: ["dsh"],
      link: "junction",
      destinations: [],
      cleanup: null,
    });
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("tab", { name: "概览" });
    fireEvent.click(screen.getByRole("button", { name: "使用…" }));
    const form = await screen.findByRole("dialog", { name: /使用 alpha/ });
    fireEvent.click(within(form).getByRole("button", { name: "选择…" }));
    expect(await within(form).findByText("D:/work/alpha")).toBeTruthy();

    fireEvent.click(within(form).getByRole("checkbox", { name: /DeepSeek/ }));
    fireEvent.click(within(form).getByRole("button", { name: "接入项目" }));
    await waitFor(() =>
      expect(mocks.attachSkill).toHaveBeenCalledWith(
        "alpha",
        null,
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });

  it("使用表单可选固定版本", async () => {
    mocks.agents.mockResolvedValue([
      {
        key: "dsh",
        display_name: "DeepSeek Harness",
        skills_dir: ".dsh/skills",
        source: "built_in",
        usages: [],
      },
    ]);
    dialog.open.mockResolvedValue("D:/work/alpha");
    mocks.attachSkill.mockResolvedValue({
      key: "alpha",
      version: "1.0.0",
      project: "D:/work/alpha",
      agents: ["dsh"],
      link: "junction",
      destinations: [],
      cleanup: null,
    });
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("tab", { name: "概览" });
    fireEvent.click(screen.getByRole("button", { name: "使用…" }));
    const form = await screen.findByRole("dialog", { name: /使用 alpha/ });
    fireEvent.click(within(form).getByRole("button", { name: "选择…" }));
    await within(form).findByText("D:/work/alpha");
    fireEvent.change(within(form).getByLabelText("版本"), {
      target: { value: "1.0.0" },
    });
    fireEvent.click(within(form).getByRole("checkbox", { name: /DeepSeek/ }));
    fireEvent.click(within(form).getByRole("button", { name: "接入项目" }));
    await waitFor(() =>
      expect(mocks.attachSkill).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });

  it("使用 tab 列出项目落点，缺失标出并可解除", async () => {
    mocks.projects.mockResolvedValue([
      {
        path: "/p/demo",
        last_used: now,
        skills: [
          {
            key: "alpha",
            version: null,
            agents: ["dsh"],
            link: "junction",
            destinations: [
              {
                path: "/p/demo/.dsh/skills/alpha",
                agents: ["dsh"],
                exists: false,
              },
            ],
          },
        ],
      },
    ]);
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    fireEvent.click(await screen.findByRole("tab", { name: "使用" }));
    expect(await screen.findByText("缺失")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "解除" }));
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));
    await waitFor(() =>
      expect(mocks.detachSkill).toHaveBeenCalledWith("alpha", "/p/demo"),
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

  it("版本 tab 列出时间线，可对比上一版并看到文件差异", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("button", { name: "发版" });
    fireEvent.click(screen.getByRole("tab", { name: "版本" }));
    expect((await screen.findAllByText("v1.1.0")).length).toBeGreaterThan(0);
    expect(screen.getByText("当前")).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", { name: /对比 v1\.0\.0 与 v1\.1\.0/ }),
    );
    await waitFor(() =>
      expect(mocks.diffVersions).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        "1.1.0",
      ),
    );
    expect((await screen.findAllByText("SKILL.md")).length).toBeGreaterThan(0);
  });

  it("归档先弹确认框，确认后才调用桥", async () => {
    renderPage();
    await screen.findByRole("region", { name: "技能详情" });

    fireEvent.click(await screen.findByRole("button", { name: "归档" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("归档技能")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "归档" }));
    await waitFor(() =>
      expect(mocks.archiveSkill).toHaveBeenCalledWith("alpha"),
    );
  });

  it("发版：更旧版本不能覆盖，发布键禁用", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "发版" }));
    const dialog = await screen.findByRole("dialog");

    fireEvent.change(screen.getByLabelText("版本号"), {
      target: { value: "1.0.0" },
    });
    expect(within(dialog).getByText(/只能覆盖最新版 v1\.1\.0/)).toBeTruthy();
    expect(
      (
        within(dialog).getByRole("button", {
          name: "发布",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  it("发版：目标版本已存在时先确认将覆盖", async () => {
    renderPage();
    await screen.findByRole("region", { name: "技能详情" });

    fireEvent.click(await screen.findByRole("button", { name: "发版" }));
    const dialog = await screen.findByRole("dialog");
    const input = screen.getByLabelText("版本号") as HTMLInputElement;
    expect(input.value).toBe("");

    fireEvent.change(input, { target: { value: "1.1.0" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    expect(within(dialog).getByText(/已存在，将覆盖/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "覆盖并发布" }));
    await waitFor(() =>
      expect(mocks.releaseSkill).toHaveBeenCalledWith("alpha", "1.1.0", true),
    );
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
