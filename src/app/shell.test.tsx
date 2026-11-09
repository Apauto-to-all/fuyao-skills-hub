import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "@/App";
import { agentRow, skillDetail, skillRow } from "@/test/fixtures";
import { testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

const readyCapabilities = {
  hub_root: "C:\\Users\\demo\\.fuyao-skills-hub",
  initialized: true,
  app_version: "0.1.0",
  platform: "windows",
};

describe("应用壳", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      capabilities: readyCapabilities,
      listSkills: [],
      showSkill: skillDetail({
        frontmatter: null,
        timeline: [],
        destination_count: null,
        content: null,
      }),
      projects: [],
      agents: [agentRow({ global_skills_dir: undefined })],
      doctor: { checks: [] },
      gc: { items: [], freed_bytes: 0 },
    });
  });

  it("未初始化时给引导并能就地初始化", async () => {
    testApi.stub({
      capabilities: { ...readyCapabilities, initialized: false },
      initHub: { initialized: true },
    });

    render(<App />);

    expect(await screen.findByText("还没有数据目录")).toBeTruthy();
    expect(screen.getByText("C:\\Users\\demo\\.fuyao-skills-hub")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "初始化" }));
    await waitFor(() => expect(testApi.raw.initHub).toHaveBeenCalledTimes(1));
  });

  it("四页导航可切换", async () => {
    render(<App />);
    expect(await screen.findByText("库里还没有技能")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "项目" }));
    expect(await screen.findByText("还没有项目用过技能")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "健康" }));
    expect(await screen.findByText("没有可回收的内容")).toBeTruthy();
  });

  it("Ctrl+K 打开命令面板并跳到目标页", async () => {
    render(<App />);
    await screen.findByText("库里还没有技能");

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const dialog = await screen.findByRole("dialog", { name: "命令面板" });

    // cmdk 的选项是 option 角色；只在对话框范围内取，别去碰侧栏那个
    fireEvent.click(within(dialog).getByRole("option", { name: "Agent" }));

    expect(
      await screen.findByRole("heading", { name: "DeepSeek Harness" }),
    ).toBeTruthy();
  });

  it("Agent 组只在面板里输入关键词后出现", async () => {
    render(<App />);
    await screen.findByText("库里还没有技能");

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const dialog = await screen.findByRole("dialog", { name: "命令面板" });
    // 空查询时列表里不出现 agent
    expect(within(dialog).queryByText("DeepSeek Harness")).toBeNull();

    fireEvent.change(within(dialog).getByPlaceholderText("搜索页面或技能…"), {
      target: { value: "deep" },
    });
    expect(await within(dialog).findByText("DeepSeek Harness")).toBeTruthy();
  });

  it("打开数据目录不改读侧数据：成功也不重拉查询", async () => {
    render(<App />);
    await screen.findByText("库里还没有技能");

    const reads = testApi.raw.capabilities.mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "打开数据目录" }));
    await waitFor(() => expect(testApi.raw.openHubDir).toHaveBeenCalled());
    // 打开目录只调桥，界面数据保持原样
    expect(testApi.raw.capabilities.mock.calls.length).toBe(reads);
  });

  it("打开数据目录失败时给提示", async () => {
    testApi.raw.openHubDir.mockRejectedValueOnce({
      code: "io",
      message: "目录不存在",
    });
    render(<App />);
    await screen.findByText("库里还没有技能");

    fireEvent.click(screen.getByRole("button", { name: "打开数据目录" }));
    expect(await screen.findByText("无法打开数据目录")).toBeTruthy();
  });

  it("Ctrl+K 能跳到某个技能", async () => {
    testApi.stub({
      listSkills: [
        skillRow({
          source: null,
          description: "甲技能",
          tags: [],
          updated: null,
        }),
      ],
    });
    render(<App />);
    await screen.findByText("alpha");

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const dialog = await screen.findByRole("dialog", { name: "命令面板" });
    fireEvent.click(within(dialog).getByRole("option", { name: /alpha/ }));

    expect(await screen.findByRole("heading", { name: "alpha" })).toBeTruthy();
  });
});
