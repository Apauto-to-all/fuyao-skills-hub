import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LibraryPage } from "@/app/library";
import { selectRows } from "@/app/library/parts";
import type { SkillSummary, SkillUsage } from "@/lib/ipc";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

// 目录选择器替换成可控桩，不启动 Tauri
const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

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
    usage: [],
    destination_count: key === "alpha" ? 1 : null,
    content: `# ${key}\n`,
  };
}

function renderPage(focusKey?: { key: string; seq: number } | null) {
  return renderWithQuery(<LibraryPage focusKey={focusKey} />);
}

/** 三条使用记录：两条跟最新、一条固定 v1.0.0，全局与项目各一处 */
const threeUsages: SkillUsage[] = [
  { scope: "project", project: "/p/one", agents: ["dsh"], version: null },
  { scope: "project", project: "/p/two", agents: ["dsh"], version: "1.0.0" },
  { scope: "global", agents: ["universal"], version: null },
];

/** 让 alpha 的详情带上指定使用记录，其余字段照旧 */
function showAlphaWithUsage(usage: SkillUsage[]) {
  testApi.raw.showSkill.mockImplementation(() =>
    Promise.resolve({
      ...detailFor("alpha", {
        name: "alpha",
        description: "甲技能",
        version: "1.1.0",
        source: "owner/repo",
        tags: ["rust"],
      }),
      usage,
    }),
  );
}

/** 选中 alpha 并切到版本 tab */
async function openVersionsTab() {
  await screen.findByRole("button", { name: /alpha/ });
  fireEvent.click(await screen.findByRole("tab", { name: "版本" }));
}

/** 点某一行的「切到这一版」，等出记录勾选框 */
async function openUpgradeDialog(version: string) {
  fireEvent.click(
    await screen.findByRole("button", { name: `切到 v${version}` }),
  );
  return screen.findByRole("dialog", { name: `切换到 v${version}` });
}

describe("库页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      listSkills: [alpha, beta],
      projects: [],
      agents: [],
      diffVersions: [
        { path: "SKILL.md", patch: "@@ -1,2 +1,2 @@\n-旧描述\n+甲技能" },
      ],
      releaseSkill: undefined,
      archiveSkill: undefined,
      restoreSkill: undefined,
      removeSkill: undefined,
      detachSkill: {
        key: "alpha",
        targets: [
          { scope: "project", project: "/p/demo", removed: [], actions: [] },
        ],
      },
      addSkill: { key: "demo", version: "0.1.0" },
    });
    testApi.raw.showSkill.mockImplementation((key: string) =>
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
    testApi.raw.releaseSkill.mockImplementation(
      (_key: string, version: string) => Promise.resolve(version),
    );
  });

  it("空库点「从目录入库」调用 add_skill", async () => {
    testApi.stub({ listSkills: [] });
    dialog.open.mockResolvedValue("/tmp/demo");
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "从目录入库…" }));
    await waitFor(() =>
      expect(testApi.raw.addSkill).toHaveBeenCalledWith("/tmp/demo"),
    );
  });

  // 滚动契约（跳转居中）在 use-jump-scroll 上测；这里只摆位：
  // 带定位键时清掉筛选并选中目标技能
  it("带定位键时选中目标技能", async () => {
    renderPage({ key: "beta", seq: 1 });
    expect(await screen.findByRole("heading", { name: "beta" })).toBeTruthy();
  });

  it("使用表单选目录与 agent 后提交", async () => {
    testApi.stub({
      agents: [
        {
          key: "dsh",
          display_name: "DeepSeek Harness",
          skills_dir: ".dsh/skills",
          source: "built_in",
          usages: [],
        },
      ],
      attachSkill: {
        key: "alpha",
        version: null,
        targets: [
          {
            scope: "project",
            project: "D:/work/alpha",
            agents: ["dsh"],
            link: "junction",
            destinations: [],
            actions: [],
          },
        ],
        cleanup: null,
      },
    });
    dialog.open.mockResolvedValue("D:/work/alpha");
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("tab", { name: "概览" });
    fireEvent.click(screen.getByRole("button", { name: "使用…" }));
    const form = await screen.findByRole("dialog", { name: /使用 alpha/ });
    fireEvent.click(within(form).getByRole("button", { name: "选择…" }));
    expect(await within(form).findByText("D:/work/alpha")).toBeTruthy();

    fireEvent.click(within(form).getByRole("checkbox", { name: /DeepSeek/ }));
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));
    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        null,
        false,
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });

  it("使用表单只勾全局时不带项目目录", async () => {
    testApi.stub({
      agents: [
        {
          key: "dsh",
          display_name: "DeepSeek Harness",
          skills_dir: ".dsh/skills",
          global_skills_dir: "C:/Users/me/.dsh/skills",
          source: "built_in",
          usages: [],
        },
      ],
      attachSkill: {
        key: "alpha",
        version: null,
        targets: [
          {
            scope: "global",
            agents: ["dsh"],
            link: "junction",
            destinations: [],
            actions: [],
          },
        ],
        cleanup: null,
      },
    });
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("tab", { name: "概览" });
    fireEvent.click(screen.getByRole("button", { name: "使用…" }));
    const form = await screen.findByRole("dialog", { name: /使用 alpha/ });
    fireEvent.click(
      within(form).getByRole("checkbox", { name: /装到全局技能目录/ }),
    );
    // 勾上全局后 agent 行改显全局位，避免选到没有全局位的 agent
    expect(within(form).getByText("C:/Users/me/.dsh/skills")).toBeTruthy();
    fireEvent.click(within(form).getByRole("checkbox", { name: /DeepSeek/ }));
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));
    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        null,
        true,
        null,
        ["dsh"],
      ),
    );
  });

  it("使用表单两者都选时项目与全局一起提交", async () => {
    testApi.stub({
      agents: [
        {
          key: "dsh",
          display_name: "DeepSeek Harness",
          skills_dir: ".dsh/skills",
          global_skills_dir: "C:/Users/me/.dsh/skills",
          source: "built_in",
          usages: [],
        },
      ],
      attachSkill: {
        key: "alpha",
        version: null,
        targets: [
          {
            scope: "global",
            agents: ["dsh"],
            link: "junction",
            destinations: [],
            actions: [],
          },
          {
            scope: "project",
            project: "D:/work/alpha",
            agents: ["dsh"],
            link: "junction",
            destinations: [],
            actions: [],
          },
        ],
        cleanup: null,
      },
    });
    dialog.open.mockResolvedValue("D:/work/alpha");
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("tab", { name: "概览" });
    fireEvent.click(screen.getByRole("button", { name: "使用…" }));
    const form = await screen.findByRole("dialog", { name: /使用 alpha/ });
    fireEvent.click(within(form).getByRole("button", { name: "选择…" }));
    await within(form).findByText("D:/work/alpha");
    fireEvent.click(
      within(form).getByRole("checkbox", { name: /装到全局技能目录/ }),
    );
    fireEvent.click(within(form).getByRole("checkbox", { name: /DeepSeek/ }));
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));
    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        null,
        true,
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });

  it("使用表单一个目标都不给时不可提交并说明原因", async () => {
    testApi.stub({
      agents: [
        {
          key: "dsh",
          display_name: "DeepSeek Harness",
          skills_dir: ".dsh/skills",
          source: "built_in",
          usages: [],
        },
      ],
    });
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("tab", { name: "概览" });
    fireEvent.click(screen.getByRole("button", { name: "使用…" }));
    const form = await screen.findByRole("dialog", { name: /使用 alpha/ });
    fireEvent.click(within(form).getByRole("checkbox", { name: /DeepSeek/ }));

    expect(within(form).getByText(/至少选一个目标/)).toBeTruthy();
    expect(
      (within(form).getByRole("button", { name: "接入" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("使用表单可选固定版本", async () => {
    testApi.stub({
      agents: [
        {
          key: "dsh",
          display_name: "DeepSeek Harness",
          skills_dir: ".dsh/skills",
          source: "built_in",
          usages: [],
        },
      ],
      attachSkill: {
        key: "alpha",
        version: "1.0.0",
        targets: [
          {
            scope: "project",
            project: "D:/work/alpha",
            agents: ["dsh"],
            link: "junction",
            destinations: [],
            actions: [],
          },
        ],
        cleanup: null,
      },
    });
    dialog.open.mockResolvedValue("D:/work/alpha");
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
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));
    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        false,
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });

  it("移除要二次确认并列出影响面", async () => {
    renderPage();
    await screen.findByRole("heading", { name: "alpha" });

    fireEvent.click(screen.getByRole("button", { name: "移除" }));
    const dialog = await screen.findByRole("dialog", { name: "移除技能" });
    expect(within(dialog).getByText(/2 个版本 tag/)).toBeTruthy();
    expect(within(dialog).getByText(/此操作不可撤销/)).toBeTruthy();

    fireEvent.click(within(dialog).getByRole("button", { name: "移除" }));
    await waitFor(() =>
      expect(testApi.raw.removeSkill).toHaveBeenCalledWith("alpha"),
    );
  });

  it("使用 tab 一条记录一行，按作用域分组，项目用法可解除", async () => {
    testApi.raw.showSkill.mockImplementation(() =>
      Promise.resolve({
        ...detailFor("alpha", {
          name: "alpha",
          description: "甲技能",
          version: "1.1.0",
          source: "owner/repo",
          tags: ["rust"],
        }),
        usage: [
          {
            scope: "project",
            project: "/p/demo",
            agents: ["dsh"],
            version: null,
          },
          {
            scope: "global",
            agents: ["universal", "dsh"],
            version: "1.1.0",
          },
        ],
      }),
    );
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    fireEvent.click(await screen.findByRole("tab", { name: "使用" }));
    // 项目段：作用域是项目，跟最新时版本如实写「跟随最新」
    const projectSection = await screen.findByRole("region", {
      name: "项目使用",
    });
    expect(within(projectSection).getByText("/p/demo")).toBeTruthy();
    expect(within(projectSection).getByText("跟随最新")).toBeTruthy();
    // 全局段：没有项目，登记版本照实给出；全局没有本地解除入口
    const globalSection = screen.getByRole("region", { name: "全局使用" });
    expect(within(globalSection).getByText("全局")).toBeTruthy();
    expect(within(globalSection).getByText("v1.1.0")).toBeTruthy();
    expect(
      within(globalSection).queryByRole("button", { name: "解除" }),
    ).toBeNull();

    fireEvent.click(
      within(projectSection).getByRole("button", { name: "解除" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });
    // agent 配置里没有这个键时，勾选项退回键本身
    expect(within(dialog).getByRole("checkbox", { name: "dsh" })).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));
    // 默认全选：桥收到这条记录登记的全部 agent
    await waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith("alpha", "/p/demo", [
        "dsh",
      ]),
    );
  });

  it("清单可见，选中行带着详情", async () => {
    renderPage();

    expect(await screen.findByRole("button", { name: /alpha/ })).toBeTruthy();
    // 描述同时出现在清单行与详情概览：限域断言详情一侧
    const detail = screen.getByRole("region", { name: "技能详情" });
    expect(await within(detail).findByText("甲技能")).toBeTruthy();
    expect(testApi.raw.showSkill).toHaveBeenCalledWith("alpha");
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
      expect(testApi.raw.diffVersions).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        "1.1.0",
      ),
    );
    expect((await screen.findAllByText("SKILL.md")).length).toBeGreaterThan(0);
  });

  it("版本 tab 每行标出在用记录数，跟最新记录归入最新行", async () => {
    showAlphaWithUsage(threeUsages);
    renderPage();
    await openVersionsTab();

    const timeline = await screen.findByRole("list", { name: "版本时间线" });
    const rowFor = (version: string) =>
      within(timeline).getByText(`v${version}`).closest("li") as HTMLElement;
    // 两条跟最新的记录计入最新一行，固定版本的那条落在自己那一行
    expect(within(rowFor("1.1.0")).getByText("2 处在用")).toBeTruthy();
    expect(within(rowFor("1.0.0")).getByText("1 处在用")).toBeTruthy();
  });

  it("切到这一版：默认全选，确认后一次调用带走选中的全局与项目", async () => {
    showAlphaWithUsage(threeUsages);
    testApi.stub({
      upgradeSkill: {
        key: "alpha",
        version: "1.0.0",
        targets: [
          {
            scope: "project",
            project: "/p/one",
            agents: ["dsh"],
            from: null,
            idempotent: false,
            destinations: [],
            actions: [],
          },
        ],
        cleanup: null,
      },
    });
    renderPage();
    await openVersionsTab();

    const dialog = await openUpgradeDialog("1.0.0");
    // 默认全选，每行写清目标、当前版本与 agent
    const boxes = within(dialog).getAllByRole("checkbox") as HTMLInputElement[];
    expect(boxes).toHaveLength(3);
    expect(boxes.every((box) => box.checked)).toBe(true);
    // 每行写清作用域、目标、当前版本与 agent
    expect(within(dialog).getAllByText("项目")).toHaveLength(2);
    expect(within(dialog).getByText("/p/two")).toBeTruthy();
    expect(within(dialog).getByText("全局")).toBeTruthy();
    expect(within(dialog).getAllByText("跟随最新")).toHaveLength(2);

    // 去掉一个项目：提交的是剩下的项目加全局，一次调用带全
    fireEvent.click(within(dialog).getByRole("checkbox", { name: /\/p\/two/ }));
    const readsBefore = testApi.raw.showSkill.mock.calls.length;
    fireEvent.click(within(dialog).getByRole("button", { name: "切换" }));
    await waitFor(() =>
      expect(testApi.raw.upgradeSkill).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        true,
        ["/p/one"],
      ),
    );
    // 成功后详情重拉：写入口统一失效已挂载的查询
    await waitFor(() =>
      expect(testApi.raw.showSkill.mock.calls.length).toBeGreaterThan(
        readsBefore,
      ),
    );
  });

  it("切到这一版：一条都不勾时确认禁用并说明原因", async () => {
    showAlphaWithUsage(threeUsages);
    renderPage();
    await openVersionsTab();

    const dialog = await openUpgradeDialog("1.0.0");
    for (const box of within(dialog).getAllByRole("checkbox")) {
      fireEvent.click(box);
    }

    expect(within(dialog).getByText("至少勾选一条使用记录")).toBeTruthy();
    expect(
      (
        within(dialog).getByRole("button", {
          name: "切换",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(testApi.raw.upgradeSkill).not.toHaveBeenCalled();
  });

  it("没有任何使用记录时切版本按钮禁用并说明原因", async () => {
    renderPage();
    await openVersionsTab();

    expect(
      await screen.findByText("还没有被任何项目或全局使用，无法切换版本"),
    ).toBeTruthy();
    const buttons = await screen.findAllByRole("button", { name: /切到 v/ });
    expect(buttons).toHaveLength(2);
    for (const button of buttons) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it("归档先弹确认框，确认后才调用桥", async () => {
    renderPage();
    await screen.findByRole("region", { name: "技能详情" });

    fireEvent.click(await screen.findByRole("button", { name: "归档" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("归档技能")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "归档" }));
    await waitFor(() =>
      expect(testApi.raw.archiveSkill).toHaveBeenCalledWith("alpha"),
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
      expect(testApi.raw.releaseSkill).toHaveBeenCalledWith(
        "alpha",
        "1.1.0",
        true,
      ),
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
