import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProjectsPage } from "@/app/projects";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

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

describe("项目页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      projects: [first, second],
      detachSkill: { key: "beta", project: "/p/two", removed: [] },
    });
  });

  it("空登记给说明", async () => {
    testApi.stub({ projects: [] });
    renderWithQuery(<ProjectsPage />);
    expect(await screen.findByText("还没有项目用过技能")).toBeTruthy();
  });

  it("点行切换检查器，缺落点标出，可就地解除", async () => {
    renderWithQuery(<ProjectsPage />);

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
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith("beta", "/p/two"),
    );
  });
});
