import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentsPage } from "@/app/agents";
import { LibraryPage } from "@/app/library";
import { ProjectsPage } from "@/app/projects";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

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
  frontmatter_error: null,
  timeline: [],
  usage: [],
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
      link_label: "目录联接（Junction）",
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
    testApi.stub({
      listSkills: [alpha],
      showSkill: detail,
      projects: [project],
      agents: [],
      detachSkill: {
        key: "alpha",
        targets: [
          { scope: "project", project: "/p/one", removed: [], actions: [] },
        ],
      },
    });
  });

  it("项目页解除使用后，库页详情与项目页本身都重新拉取", async () => {
    // 首次读取给「在用」，撤回后给「已撤空」：重拉与不重拉在界面上一眼可分
    testApi.raw.projects
      .mockResolvedValueOnce([project])
      .mockResolvedValue([{ ...project, skills: [] }]);

    const { client } = renderWithQuery(
      <>
        <div data-testid="projects">
          <ProjectsPage />
        </div>
        <div data-testid="library">
          <LibraryPage />
        </div>
      </>,
    );
    expect(client).toBeTruthy();

    await waitFor(() => expect(testApi.raw.showSkill).toHaveBeenCalledTimes(1));

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
    expect(testApi.raw.showSkill).toHaveBeenCalledTimes(2);
  });

  it("库页接入全局目标后，项目页与 Agent 页都重新拉取", async () => {
    const agent = {
      key: "dsh",
      display_name: "DeepSeek Harness",
      skills_dir: ".dsh/skills",
      global_skills_dir: "/home/me/.dsh/skills",
      source: "built_in" as const,
      usages: [],
    };
    testApi.stub({
      agents: [agent],
      attachSkill: {
        key: "alpha",
        version: null,
        targets: [
          {
            scope: "global",
            agents: ["dsh"],
            link: "junction",
            destinations: [],
            actions: [],
          },
        ],
        cleanup: null,
      },
    });

    renderWithQuery(
      <>
        <ProjectsPage />
        <AgentsPage />
        <LibraryPage />
      </>,
    );

    await waitFor(() => expect(testApi.raw.agents).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(testApi.raw.projects).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(testApi.raw.showSkill).toHaveBeenCalledTimes(1));

    // 只勾「全局」不选目录：提交的是一份全局单目标的接入
    fireEvent.click(await screen.findByRole("button", { name: "使用…" }));
    const form = await screen.findByRole("dialog", { name: /使用 alpha/ });
    fireEvent.click(
      within(form).getByRole("checkbox", { name: /装到全局技能目录/ }),
    );
    fireEvent.click(within(form).getByRole("checkbox", { name: /DeepSeek/ }));
    // 三个页面的读取次数在提交前记一笔：重拉与不重拉只差这一个增量
    const before = {
      projects: testApi.raw.projects.mock.calls.length,
      agents: testApi.raw.agents.mock.calls.length,
      detail: testApi.raw.showSkill.mock.calls.length,
    };
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));

    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        null,
        true,
        null,
        ["dsh"],
      ),
    );
    // 三页各走各的查询，读侧失效后三个都涨了
    await waitFor(() =>
      expect(testApi.raw.projects.mock.calls.length).toBeGreaterThan(
        before.projects,
      ),
    );
    expect(testApi.raw.agents.mock.calls.length).toBeGreaterThan(before.agents);
    expect(testApi.raw.showSkill.mock.calls.length).toBeGreaterThan(
      before.detail,
    );
  });
});
