import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
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

function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <AgentsPage />
    </QueryClientProvider>,
  );
}

describe("Agent 页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.agents.mockResolvedValue(rows);
  });

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
});
