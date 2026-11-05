import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsPage } from "@/app/agents";

const mocks = vi.hoisted(() => ({ agents: vi.fn() }));

vi.mock("@/lib/ipc", () => ({ api: { agents: mocks.agents } }));

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

function renderPage(focusKey?: { key: string; seq: number } | null) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    client,
    ...render(
      <QueryClientProvider client={client}>
        <AgentsPage focusKey={focusKey} />
      </QueryClientProvider>,
    ),
  };
}

describe("Agent 页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.agents.mockResolvedValue(rows);
  });

  afterEach(() => vi.restoreAllMocks());

  it("表格给使用数，检查器反查项目与版本", async () => {
    renderPage();

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

  it("带定位键时选中并滚到该 agent", async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
    renderPage({ key: "universal", seq: 1 });

    const inspector = await screen.findByRole("complementary");
    expect(
      await within(inspector).findByText(/还没有项目用这个 agent/),
    ).toBeTruthy();

    const table = screen.getAllByRole("table")[0];
    const target = within(table)
      .getByRole("button", { name: /通用/ })
      .closest("tr");
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "center" });
    expect(scrollIntoView.mock.instances[0]).toBe(target);
  });

  it("同一个 agent 再跳一次仍会滚到目标", async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
    const { client, rerender } = renderPage({ key: "universal", seq: 1 });
    await screen.findByRole("complementary");
    scrollIntoView.mockClear();

    rerender(
      <QueryClientProvider client={client}>
        <AgentsPage focusKey={{ key: "universal", seq: 2 }} />
      </QueryClientProvider>,
    );

    expect(scrollIntoView).toHaveBeenCalledTimes(1);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "center" });
  });

  it("点击行只做最小滚动", async () => {
    const scrollIntoView = vi.spyOn(Element.prototype, "scrollIntoView");
    renderPage();

    const table = (await screen.findAllByRole("table"))[0];
    fireEvent.click(within(table).getByRole("button", { name: /通用/ }));

    expect(scrollIntoView).toHaveBeenLastCalledWith({ block: "nearest" });
  });
});
