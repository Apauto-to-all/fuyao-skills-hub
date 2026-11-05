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
  projects: vi.fn(),
  agents: vi.fn(),
  openHubDir: vi.fn(),
}));

vi.mock("@/lib/ipc", () => ({
  api: {
    capabilities: mocks.capabilities,
    initHub: mocks.initHub,
    listSkills: mocks.listSkills,
    projects: mocks.projects,
    agents: mocks.agents,
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
    expect(await screen.findByText("健康页")).toBeTruthy();
  });

  it("⌘K 打开命令面板并跳到目标页", async () => {
    mocks.capabilities.mockResolvedValue(readyCapabilities);

    render(<App />);
    await screen.findByText("库里还没有技能");

    fireEvent.keyDown(window, { key: "k", metaKey: true });
    const dialog = await screen.findByRole("dialog", { name: "命令面板" });

    // 面板里出现同名项：只在对话框范围内取，别去碰侧栏那个
    fireEvent.click(within(dialog).getByRole("button", { name: "Agent" }));

    expect(
      await screen.findByRole("heading", { name: "DeepSeek Harness" }),
    ).toBeTruthy();
  });
});
