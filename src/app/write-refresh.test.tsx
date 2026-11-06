import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LibraryPage } from "@/app/library";
import { ProjectsPage } from "@/app/projects";

// 替换 ipc 模块：两个页面共用一个 client，验证写操作后两边都重新拉取
const mocks = vi.hoisted(() => ({
  listSkills: vi.fn(),
  showSkill: vi.fn(),
  projects: vi.fn(),
  agents: vi.fn(),
  detachSkill: vi.fn(),
}));

const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

vi.mock("@/lib/ipc", () => ({
  api: {
    listSkills: mocks.listSkills,
    showSkill: mocks.showSkill,
    projects: mocks.projects,
    agents: mocks.agents,
    detachSkill: mocks.detachSkill,
  },
  isGuiError: (value: unknown) =>
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value,
}));

const now = Math.floor(Date.now() / 1000);

const alpha = {
  key: "alpha",
  latest_version: "1.0.0",
  archived: false,
  source: null,
  description: "甲技能",
  tags: [],
  updated: now - 86_400,
};

const detail = {
  key: "alpha",
  latest_version: "1.0.0",
  archived: false,
  frontmatter: {
    name: "alpha",
    description: "甲技能",
    version: "1.0.0",
    source: null,
    tags: [],
  },
  timeline: [],
  referenced_by: [],
  destination_count: 1,
  content: "# alpha\n",
};

const project = {
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

/**
 * 写操作完成后，同时挂载的另一个页面必须重新拉取。
 * 真实的四页切换是卸载重建，这个用例锁的是写入口本身的行为：
 * 成功后让所有已挂载查询失效，调用点不再各自维护键清单。
 */
describe("写操作后的跨页刷新", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listSkills.mockResolvedValue([alpha]);
    mocks.showSkill.mockResolvedValue(detail);
    mocks.projects.mockResolvedValue([project]);
    mocks.agents.mockResolvedValue([]);
    mocks.detachSkill.mockResolvedValue({
      key: "alpha",
      project: "/p/one",
      removed: [],
    });
  });

  it("项目页解除使用后，库页详情与项目页本身都重新拉取", async () => {
    // 首次读取给「在用」，撤回后给「已撤空」：重拉与不重拉在界面上一眼可分
    mocks.projects
      .mockResolvedValueOnce([project])
      .mockResolvedValue([{ ...project, skills: [] }]);

    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <div data-testid="projects">
          <ProjectsPage />
        </div>
        <div data-testid="library">
          <LibraryPage />
        </div>
      </QueryClientProvider>,
    );

    await waitFor(() => expect(mocks.showSkill).toHaveBeenCalledTimes(1));

    const inspector = await screen.findByRole("complementary");
    expect(within(inspector).getByText("alpha")).toBeTruthy();
    fireEvent.click(within(inspector).getByRole("button", { name: "解除" }));
    const confirm = await screen.findByRole("dialog", { name: "解除使用" });
    fireEvent.click(within(confirm).getByRole("button", { name: "解除" }));

    // 项目页：清单里那条技能消失
    await waitFor(() =>
      expect(within(inspector).getByText("这个项目没有登记技能")).toBeTruthy(),
    );
    // 库页：详情查询跟着重拉，不是停留在旧值
    expect(mocks.showSkill).toHaveBeenCalledTimes(2);
  });
});
