import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/App";

// 替换 ipc 模块，固定返回值，不启动 Tauri
const mocks = vi.hoisted(() => ({
  capabilities: vi.fn(),
  initHub: vi.fn(),
  listSkills: vi.fn(),
  showSkill: vi.fn(),
  projects: vi.fn(),
  agents: vi.fn(),
  doctor: vi.fn(),
  gc: vi.fn(),
  openHubDir: vi.fn(),
}));

vi.mock("@/lib/ipc", () => ({
  api: {
    capabilities: mocks.capabilities,
    initHub: mocks.initHub,
    listSkills: mocks.listSkills,
    showSkill: mocks.showSkill,
    projects: mocks.projects,
    agents: mocks.agents,
    doctor: mocks.doctor,
    gc: mocks.gc,
    openHubDir: mocks.openHubDir,
  },
  isGuiError: (value: unknown) =>
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value,
}));

const readyCapabilities = {
  hub_root: "D:\\fuyao\\data",
  initialized: true,
  app_version: "0.1.0",
  platform: "windows",
};

describe("应用壳", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listSkills.mockResolvedValue([]);
    mocks.projects.mockResolvedValue([]);
    mocks.agents.mockResolvedValue([
      {
        key: "dsh",
        display_name: "DeepSeek Harness",
        skills_dir: ".dsh/skills",
        source: "built_in",
        usages: [],
      },
    ]);
    mocks.doctor.mockResolvedValue({ checks: [] });
    mocks.gc.mockResolvedValue({ items: [], freed_bytes: 0 });
    mocks.showSkill.mockResolvedValue({
      key: "alpha",
      latest_version: "1.0.0",
      archived: false,
      frontmatter: null,
      timeline: [],
      referenced_by: [],
      destination_count: null,
      content: null,
    });
  });

  it("未初始化时给引导并能就地初始化", async () => {
    mocks.capabilities.mockResolvedValue({
      ...readyCapabilities,
      initialized: false,
    });
    mocks.initHub.mockResolvedValue({ initialized: true });

    render(<App />);

    expect(await screen.findByText("还没有数据目录")).toBeTruthy();
    expect(screen.getByText("D:\\fuyao\\data")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "初始化" }));
    await waitFor(() => expect(mocks.initHub).toHaveBeenCalledTimes(1));
  });

  it("四页导航可切换", async () => {
    mocks.capabilities.mockResolvedValue(readyCapabilities);

    render(<App />);
    expect(await screen.findByText("库里还没有技能")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "项目" }));
    expect(await screen.findByText("还没有项目用过技能")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "健康" }));
    expect(await screen.findByText("没有可回收的内容")).toBeTruthy();
  });

  it("⌘K 打开命令面板并跳到目标页", async () => {
    mocks.capabilities.mockResolvedValue(readyCapabilities);

    render(<App />);
    await screen.findByText("库里还没有技能");

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const dialog = await screen.findByRole("dialog", { name: "命令面板" });

    // cmdk 的选项是 option 角色；只在对话框范围内取，别去碰侧栏那个
    fireEvent.click(within(dialog).getByRole("option", { name: "Agent" }));

    expect(
      await screen.findByRole("heading", { name: "DeepSeek Harness" }),
    ).toBeTruthy();
  });

  it("Agent 组只在面板里输入关键词后出现", async () => {
    mocks.capabilities.mockResolvedValue(readyCapabilities);

    render(<App />);
    await screen.findByText("库里还没有技能");

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const dialog = await screen.findByRole("dialog", { name: "命令面板" });
    // 空查询时列表里不出现 agent
    expect(within(dialog).queryByText("DeepSeek Harness")).toBeNull();

    fireEvent.change(within(dialog).getByPlaceholderText("搜索页面或技能…"), {
      target: { value: "deep" },
    });
    expect(await within(dialog).findByText("DeepSeek Harness")).toBeTruthy();
  });

  it("打开数据目录失败时给提示", async () => {
    mocks.openHubDir.mockRejectedValue({ code: "io", message: "目录不存在" });
    render(<App />);
    await screen.findByText("库里还没有技能");

    fireEvent.click(screen.getByRole("button", { name: "打开数据目录" }));
    expect(await screen.findByText("无法打开数据目录")).toBeTruthy();
  });

  it("⌘K 能跳到某个技能", async () => {
    mocks.listSkills.mockResolvedValue([
      {
        key: "alpha",
        latest_version: "1.0.0",
        archived: false,
        source: null,
        description: "甲技能",
        tags: [],
        updated: null,
      },
    ]);
    render(<App />);
    await screen.findByText("alpha");

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const dialog = await screen.findByRole("dialog", { name: "命令面板" });
    fireEvent.click(within(dialog).getByRole("option", { name: /alpha/ }));

    expect(await screen.findByRole("heading", { name: "alpha" })).toBeTruthy();
  });
});
