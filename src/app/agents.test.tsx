import { fireEvent, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsPage } from "@/app/agents";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

const rows = [
  {
    key: "dsh",
    display_name: "DeepSeek Harness",
    skills_dir: ".dsh/skills",
    global_skills_dir: "C:/Users/me/.dsh/skills",
    source: "built_in",
    usages: [
      { scope: "project", project: "/p/one", key: "alpha", version: null },
      { scope: "project", project: "/p/two", key: "beta", version: "2.0.0" },
      { scope: "global", key: "gamma", version: null },
    ],
  },
  {
    key: "universal",
    display_name: "通用",
    skills_dir: ".agents/skills",
    global_skills_dir: "C:/Users/me/.agents/skills",
    source: "user",
    usages: [],
  },
  {
    key: "globalsonly",
    display_name: "只有全局位",
    global_skills_dir: "C:/Users/me/.globalsonly/skills",
    source: "user",
    usages: [],
  },
];

describe("Agent 页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({ agents: rows });
  });

  afterEach(() => vi.restoreAllMocks());

  it("表格给使用数，检查器按项目与全局两段反查", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    expect(within(table).getByText("DeepSeek Harness")).toBeTruthy();
    // 三条夹具里两条是用户覆盖，这里只断言来源列译成了中文
    expect(within(table).getAllByText("用户覆盖").length).toBeGreaterThan(0);

    fireEvent.click(
      within(table).getByRole("button", { name: /DeepSeek Harness/ }),
    );
    const inspector = screen.getByRole("complementary");
    const projectSection = await within(inspector).findByRole("region", {
      name: "项目用法",
    });
    expect(within(projectSection).getByText("/p/two")).toBeTruthy();
    expect(within(projectSection).getByText(/v2\.0\.0/)).toBeTruthy();

    const globalSection = within(inspector).getByRole("region", {
      name: "全局用法",
    });
    expect(within(globalSection).getByText(/gamma/)).toBeTruthy();
  });

  it("两个目录都显示展开后的绝对路径", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    fireEvent.click(
      within(table).getByRole("button", { name: /DeepSeek Harness/ }),
    );
    const inspector = screen.getByRole("complementary");
    expect(
      await within(inspector).findByText("C:/Users/me/.dsh/skills"),
    ).toBeTruthy();
    expect(within(inspector).getByText(".dsh/skills")).toBeTruthy();
  });

  it("空段各自给一句说明", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    fireEvent.click(within(table).getByRole("button", { name: /通用/ }));
    const inspector = screen.getByRole("complementary");
    const projectSection = await within(inspector).findByRole("region", {
      name: "项目用法",
    });
    expect(
      within(projectSection).getByText(/还没有项目用这个 agent/),
    ).toBeTruthy();
    const globalSection = within(inspector).getByRole("region", {
      name: "全局用法",
    });
    expect(
      within(globalSection).getByText(/还没有技能装到全局技能目录/),
    ).toBeTruthy();
  });

  it("只有全局位的 agent 不显示项目目录", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    fireEvent.click(within(table).getByRole("button", { name: /只有全局位/ }));
    const inspector = screen.getByRole("complementary");
    expect(
      await within(inspector).findByText("C:/Users/me/.globalsonly/skills"),
    ).toBeTruthy();
    expect(within(inspector).queryByText("项目目录")).toBeNull();
  });

  // 滚动契约（跳转居中 / 重跳 / 点击最小滚动）在 use-jump-scroll 上测，
  // 这里只摆位：带定位键时目标 agent 进检查器
  it("带定位键时选中目标 agent", async () => {
    renderWithQuery(<AgentsPage focusKey={{ key: "universal", seq: 1 }} />);

    const inspector = await screen.findByRole("complementary");
    expect(
      await within(inspector).findByRole("region", { name: "项目用法" }),
    ).toBeTruthy();
  });
});
