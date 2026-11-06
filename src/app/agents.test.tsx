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
    source: "built_in",
    usages: [
      { project: "/p/one", key: "alpha", version: null },
      { project: "/p/two", key: "beta", version: "2.0.0" },
    ],
  },
  {
    key: "universal",
    display_name: "通用",
    skills_dir: ".agents/skills",
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

  it("表格给使用数，检查器反查项目与版本", async () => {
    renderWithQuery(<AgentsPage />);

    const table = (await screen.findAllByRole("table"))[0];
    expect(within(table).getByText("DeepSeek Harness")).toBeTruthy();
    expect(within(table).getByText("用户覆盖")).toBeTruthy();

    fireEvent.click(within(table).getByRole("button", { name: /通用/ }));
    const inspector = screen.getByRole("complementary");
    expect(
      await within(inspector).findByText(/还没有项目用这个 agent/),
    ).toBeTruthy();

    fireEvent.click(
      within(table).getByRole("button", { name: /DeepSeek Harness/ }),
    );
    expect(await within(inspector).findByText("/p/two")).toBeTruthy();
    expect(within(inspector).getByText(/v2\.0\.0/)).toBeTruthy();
  });

  // 滚动契约（跳转居中 / 重跳 / 点击最小滚动）在 use-jump-scroll 上测，
  // 这里只摆位：带定位键时目标 agent 进检查器
  it("带定位键时选中目标 agent", async () => {
    renderWithQuery(<AgentsPage focusKey={{ key: "universal", seq: 1 }} />);

    const inspector = await screen.findByRole("complementary");
    expect(
      await within(inspector).findByText(/还没有项目用这个 agent/),
    ).toBeTruthy();
  });
});
