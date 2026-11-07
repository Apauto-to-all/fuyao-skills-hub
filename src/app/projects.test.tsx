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
      link_label: "目录联接（Junction）",
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
      link_label: "拷贝（Copy）",
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

/** agent 配置：勾选项的显示名取自这里 */
const agentRows = [
  {
    key: "universal",
    display_name: "通用技能目录",
    skills_dir: ".agents/skills",
    source: "built_in",
    usages: [],
  },
  {
    key: "dsh",
    display_name: "DeepSeek Harness",
    skills_dir: ".dsh/skills",
    source: "built_in",
    usages: [],
  },
];

/** 两个 agent 解析到同一个目录：撤哪几个只由勾选名单决定 */
const sharedProject = {
  path: "/p/three",
  last_used: now,
  skills: [
    {
      key: "gamma",
      version: null,
      agents: ["universal", "dsh"],
      link: "junction",
      link_label: "目录联接（Junction）",
      destinations: [
        {
          path: "/p/three/.agents/skills/gamma",
          agents: ["universal", "dsh"],
          exists: true,
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
      agents: agentRows,
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

    // 默认全选：桥收到的就是这条记录登记的全部 agent
    await vi.waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith("beta", "/p/two", [
        "universal",
      ]),
    );
  });

  it("取消勾选一个 agent 时，桥只收到勾上的那些", async () => {
    testApi.stub({ projects: [sharedProject] });
    renderWithQuery(<ProjectsPage />);

    const inspector = await screen.findByRole("complementary");
    fireEvent.click(within(inspector).getByRole("button", { name: "解除" }));
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });

    // 默认全选：记录里的两个 agent 都是勾上的
    const universal = await within(dialog).findByRole("checkbox", {
      name: "通用技能目录",
    });
    const harness = within(dialog).getByRole("checkbox", {
      name: "DeepSeek Harness",
    });
    expect((universal as HTMLInputElement).checked).toBe(true);
    expect((harness as HTMLInputElement).checked).toBe(true);

    fireEvent.click(universal);
    // 文案点名这次要撤的 agent，并说明共用目录会留下
    expect(
      within(dialog).getByText(/只撤 gamma 在 DeepSeek Harness/),
    ).toBeTruthy();
    expect(
      within(dialog).getByText(/通用技能目录的落点与登记原样留下/),
    ).toBeTruthy();
    expect(
      within(dialog).getByText(/被其他 agent 共用的目录会保留/),
    ).toBeTruthy();

    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));
    await vi.waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "gamma",
        "/p/three",
        ["dsh"],
      ),
    );
  });

  it("全选确认时桥收到记录的全部 agent", async () => {
    testApi.stub({ projects: [sharedProject] });
    renderWithQuery(<ProjectsPage />);

    const inspector = await screen.findByRole("complementary");
    fireEvent.click(within(inspector).getByRole("button", { name: "解除" }));
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });

    // 点满记录时落点全删，没有留下的目录，文案不提共用目录
    expect(
      within(dialog).queryByText(/被其他 agent 共用的目录会保留/),
    ).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));
    await vi.waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "gamma",
        "/p/three",
        ["universal", "dsh"],
      ),
    );
  });

  it("全部取消勾选时确认按钮禁用并说明原因", async () => {
    testApi.stub({ projects: [sharedProject] });
    renderWithQuery(<ProjectsPage />);

    const inspector = await screen.findByRole("complementary");
    fireEvent.click(within(inspector).getByRole("button", { name: "解除" }));
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });

    fireEvent.click(
      await within(dialog).findByRole("checkbox", { name: "通用技能目录" }),
    );
    fireEvent.click(
      within(dialog).getByRole("checkbox", { name: "DeepSeek Harness" }),
    );

    const confirm = within(dialog).getByRole("button", {
      name: "解除",
    }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
    expect(
      within(dialog).getByText("至少勾选一个 agent 才能解除"),
    ).toBeTruthy();
    expect(testApi.raw.detachSkill).not.toHaveBeenCalled();
  });
});
