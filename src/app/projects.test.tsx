import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProjectsPage } from "@/app/projects";
import { agentRow, NOW, projectRow, projectSkill } from "@/test/fixtures";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

const first = projectRow("/p/one", [
  projectSkill({
    destinations: [
      { path: "/p/one/.dsh/skills/alpha", agents: ["dsh"], exists: true },
    ],
  }),
]);
const second = projectRow(
  "/p/two",
  [
    projectSkill({
      key: "beta",
      version: "2.0.0",
      agents: ["universal"],
      link: "copy",
      link_label: "拷贝（Copy）",
      destinations: [
        {
          path: "/p/two/.agents/skills/beta",
          agents: ["universal"],
          exists: false,
        },
      ],
    }),
  ],
  { last_used: NOW - 86_400 },
);

describe("项目页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      projects: [first, second],
      agents: [
        agentRow({ key: "universal", display_name: "通用技能目录" }),
        agentRow(),
      ],
      detachSkill: {
        key: "beta",
        targets: [
          { scope: "project", project: "/p/two", removed: [], actions: [] },
        ],
      },
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

    // 默认全选即整条撤完：名单省缺，桥按整体撤回处理
    await vi.waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "beta",
        false,
        ["/p/two"],
        undefined,
      ),
    );
  });
});
