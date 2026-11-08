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
    global_skills_dir: "~/.dsh/skills",
    source: "built_in",
  },
  {
    key: "universal",
    display_name: "通用",
    skills_dir: ".agents/skills",
    global_skills_dir: "~/.agents/skills",
    source: "user",
  },
  {
    key: "globalsonly",
    display_name: "只有全局位",
    global_skills_dir: "~/.globalsonly/skills",
    source: "user",
  },
];

describe("Agent 页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({ agents: rows });
  });

  afterEach(() => vi.restoreAllMocks());

  it("表格列配置：键与显示名、来源、两个技能位写配置原文", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    expect(within(table).getByText("DeepSeek Harness")).toBeTruthy();
    expect(within(table).getByText("dsh")).toBeTruthy();
    expect(within(table).getByText(".dsh/skills")).toBeTruthy();
    expect(within(table).getByText("~/.dsh/skills")).toBeTruthy();
    // 三条夹具里两条是用户覆盖，这里只断言来源列译成了中文
    expect(within(table).getAllByText("用户覆盖").length).toBeGreaterThan(0);
  });

  it("缺位写破折号，不拿另一侧目录顶上", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    const row = within(table).getByText("只有全局位").closest("tr");
    const cells = within(row as HTMLElement).getAllByRole("cell");
    expect(cells[2].textContent).toBe("—");
    expect(cells[3].textContent).toBe("~/.globalsonly/skills");
  });

  it("检查器只列选中 agent 的两个目录", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    fireEvent.click(
      within(table).getByRole("button", { name: /DeepSeek Harness/ }),
    );
    const inspector = screen.getByRole("complementary");
    expect(await within(inspector).findByText("项目目录")).toBeTruthy();
    expect(within(inspector).getByText(".dsh/skills")).toBeTruthy();
    expect(within(inspector).getByText("全局目录")).toBeTruthy();
    expect(within(inspector).getByText("~/.dsh/skills")).toBeTruthy();
  });

  it("只有全局位的 agent 不显示项目目录", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    fireEvent.click(within(table).getByRole("button", { name: /只有全局位/ }));
    const inspector = screen.getByRole("complementary");
    expect(
      await within(inspector).findByText("~/.globalsonly/skills"),
    ).toBeTruthy();
    expect(within(inspector).queryByText("项目目录")).toBeNull();
  });

  // 滚动契约（跳转居中 / 重跳 / 点击最小滚动）在 use-jump-scroll 上测，
  // 这里只摆位：带定位键时目标 agent 进检查器
  it("带定位键时选中目标 agent", async () => {
    renderWithQuery(<AgentsPage focusKey={{ key: "universal", seq: 1 }} />);

    const inspector = await screen.findByRole("complementary");
    expect(await within(inspector).findByText("通用")).toBeTruthy();
    expect(within(inspector).getByText("~/.agents/skills")).toBeTruthy();
  });
});
