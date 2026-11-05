import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProjectsPage } from "@/app/projects";

const mocks = vi.hoisted(() => ({
  projects: vi.fn(),
  detachSkill: vi.fn(),
}));

vi.mock("@/lib/ipc", () => ({
  api: { projects: mocks.projects, detachSkill: mocks.detachSkill },
  isGuiError: (value: unknown) =>
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value,
}));

const now = Math.floor(Date.now() / 1000);
const first = {
  path: "/p/one",
  last_used: now,
  skills: [
    {
      key: "alpha",
      version: null,
      agents: ["dsh"],
      link: "junction",
      destinations: [
        { path: "/p/one/.dsh/skills/alpha", agents: ["dsh"], exists: true },
      ],
    },
  ],
};
const second = {
  path: "/p/two",
  last_used: now - 86_400,
  skills: [
    {
      key: "beta",
      version: "2.0.0",
      agents: ["universal"],
      link: "copy",
      destinations: [
        {
          path: "/p/two/.agents/skills/beta",
          agents: ["universal"],
          exists: false,
        },
      ],
    },
  ],
};

function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ProjectsPage />
    </QueryClientProvider>,
  );
}

describe("项目页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.detachSkill.mockResolvedValue({
      key: "beta",
      project: "/p/two",
      removed: [],
    });
  });

  it("空登记给说明", async () => {
    mocks.projects.mockResolvedValue([]);
    renderPage();
    expect(await screen.findByText("还没有项目用过技能")).toBeTruthy();
  });

  it("点行切换检查器，缺落点标出，可就地解除", async () => {
    mocks.projects.mockResolvedValue([first, second]);
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "/p/two" }));
    const inspector = screen.getByRole("complementary");
    expect(await within(inspector).findByText("beta")).toBeTruthy();
    expect(
      within(inspector).getByText(/缺失：\/p\/two\/\.agents/),
    ).toBeTruthy();
    expect(within(inspector).getByText(/拷贝/)).toBeTruthy();

    fireEvent.click(within(inspector).getByRole("button", { name: "解除" }));
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });
    expect(within(dialog).getByText(/\/p\/two/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));

    await vi.waitFor(() =>
      expect(mocks.detachSkill).toHaveBeenCalledWith("beta", "/p/two"),
    );
  });
});
