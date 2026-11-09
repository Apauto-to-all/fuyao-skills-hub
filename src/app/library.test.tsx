import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LibraryPage } from "@/app/library";
import type { SkillUsage } from "@/lib/ipc";
import {
  globalUsage,
  projectUsage,
  skillDetail,
  skillRow,
} from "@/test/fixtures";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

// 目录选择器替换成可控桩，不启动 Tauri
const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

const alpha = skillRow();
const beta = skillRow({
  key: "beta",
  latest_version: null,
  archived: true,
  source: null,
  description: null,
  tags: [],
  updated: null,
});
const betaDetail = skillDetail({
  key: "beta",
  latest_version: null,
  archived: true,
  frontmatter: null,
  timeline: [],
  destination_count: null,
  content: "# beta\n",
});

function renderPage(focusKey?: { key: string; seq: number } | null) {
  return renderWithQuery(<LibraryPage focusKey={focusKey} />);
}

/** 让 alpha 的详情带上指定使用记录，其余字段照旧 */
function showAlphaWithUsage(usage: SkillUsage[]) {
  testApi.raw.showSkill.mockImplementation(() =>
    Promise.resolve(skillDetail({ usage })),
  );
}

/** 选中 alpha 并切到版本 tab */
async function openVersionsTab() {
  await screen.findByRole("button", { name: /alpha/ });
  fireEvent.click(await screen.findByRole("tab", { name: "版本" }));
}

describe("库页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      listSkills: [alpha, beta],
      projects: [],
      agents: [],
      diffVersions: [
        { path: "SKILL.md", patch: "@@ -1,2 +1,2 @@\n-旧描述\n+甲技能" },
      ],
      detachSkill: {
        key: "alpha",
        targets: [
          { scope: "project", project: "/p/demo", removed: [], actions: [] },
        ],
      },
      addSkill: { key: "demo", version: "0.1.0" },
    });
    testApi.raw.showSkill.mockImplementation((key: string) =>
      Promise.resolve(key === "alpha" ? skillDetail() : betaDetail),
    );
  });

  it("空库点「从目录入库」调用 add_skill", async () => {
    testApi.stub({ listSkills: [] });
    dialog.open.mockResolvedValue("/tmp/demo");
    renderPage();

    fireEvent.click(await screen.findByRole("button", { name: "从目录入库…" }));
    await waitFor(() =>
      expect(testApi.raw.addSkill).toHaveBeenCalledWith("/tmp/demo"),
    );
  });

  // 滚动契约（跳转居中）在 use-jump-scroll 上测；这里只摆位：
  // 带定位键时清掉筛选并选中目标技能
  it("带定位键时选中目标技能", async () => {
    renderPage({ key: "beta", seq: 1 });
    expect(await screen.findByRole("heading", { name: "beta" })).toBeTruthy();
  });

  it("清单可见，选中行带着详情", async () => {
    renderPage();
    expect(await screen.findByRole("button", { name: /alpha/ })).toBeTruthy();
    // 描述同时出现在清单行与详情概览：限域断言详情一侧
    const detail = screen.getByRole("region", { name: "技能详情" });
    expect(await within(detail).findByText("甲技能")).toBeTruthy();
    expect(testApi.raw.showSkill).toHaveBeenCalledWith("alpha");
  });

  it("搜索走内容检索：命中键集合过滤清单，正文中含词的技能被保留", async () => {
    testApi.stub({
      searchHub: {
        hits: [
          {
            key: "beta",
            path: "store/beta/SKILL.md",
            line: 3,
            text: "正文里有 针 词",
            spans: [[5, 6]],
            context: {
              before: [{ line: 2, text: "上一行" }],
              after: [{ line: 4, text: "下一行" }],
            },
          },
        ],
        total: 1,
        truncated: false,
      },
    });
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const search = screen.getByLabelText("搜索技能");
    fireEvent.change(search, { target: { value: "针" } });
    // beta 的键/描述都不含「针」，但正文命中即保留；alpha 不在键集合内被滤掉
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /alpha/ })).toBeNull(),
    );
    expect(screen.getByRole("button", { name: /beta/ })).toBeTruthy();
    // 命中片段渲染：定位行、上下文行与 spans 高亮
    expect(screen.getByText("store/beta/SKILL.md:3")).toBeTruthy();
    expect(screen.getByText("上一行")).toBeTruthy();
    expect(screen.getByText("下一行")).toBeTruthy();
    expect(screen.getByText("针")).toBeTruthy();
    // 检索词原样传桥，带上下文行数
    expect(testApi.raw.searchHub).toHaveBeenCalledWith("针", 1);

    // 清空查询词回到未过滤状态
    fireEvent.change(search, { target: { value: "" } });
    expect(await screen.findByRole("button", { name: /alpha/ })).toBeTruthy();
  });

  it("检索无命中给出空态，清除筛选复原", async () => {
    testApi.stub({ searchHub: { hits: [], total: 0, truncated: false } });
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const search = screen.getByLabelText("搜索技能");
    fireEvent.change(search, { target: { value: "查无此词" } });
    expect(await screen.findByText("没有匹配的技能")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "清除筛选" }));
    expect(screen.getByRole("button", { name: /alpha/ })).toBeTruthy();
  });

  it("检索失败时不过滤清单并给出行内错误", async () => {
    testApi.raw.searchHub.mockRejectedValueOnce({
      code: "usage",
      message: "检索词为空",
    });
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const search = screen.getByLabelText("搜索技能");
    fireEvent.change(search, { target: { value: "坏词" } });
    expect(await screen.findByText(/本次不过滤/)).toBeTruthy();
    // 失败时清单不收窄：alpha、beta 都在
    expect(screen.getByRole("button", { name: /alpha/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /beta/ })).toBeTruthy();
  });

  it("归档分段切「已归档」后只剩归档项", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const segments = screen.getByRole("group", { name: "归档态" });
    fireEvent.click(within(segments).getByRole("button", { name: "已归档" }));
    expect(screen.queryByRole("button", { name: /alpha/ })).toBeNull();
    expect(screen.getByRole("button", { name: /beta/ })).toBeTruthy();
  });

  it("点标签 chip 后清单按标签收敛", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    const list = screen.getByRole("region", { name: "技能列表" });
    fireEvent.click(within(list).getByRole("button", { name: "#rust" }));
    expect(screen.queryByRole("button", { name: /beta/ })).toBeNull();
    expect(screen.getByRole("button", { name: /alpha/ })).toBeTruthy();
  });

  it("frontmatter 缺失时详情顶部给黄条", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    fireEvent.click(screen.getByRole("button", { name: /beta/ }));
    expect(
      await screen.findByText(/SKILL.md 缺失或 frontmatter 无法解析/),
    ).toBeTruthy();
  });

  it("frontmatter 解析失败时黄条带出失败原因，解析成功就不带", async () => {
    const reason =
      "无效的 skill 源: store/beta/SKILL.md（frontmatter 顶层不得出现 tags：fsh 自有字段写在 metadata 下（tags: …））";
    testApi.raw.showSkill.mockImplementation((key: string) =>
      Promise.resolve(
        key === "beta"
          ? skillDetail({ ...betaDetail, frontmatter_error: reason })
          : skillDetail(),
      ),
    );
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    // 解析失败：黄条除了原话还带出点名文件与字段的原因，且是整段原文
    fireEvent.click(screen.getByRole("button", { name: /beta/ }));
    expect(
      await screen.findByText(/SKILL.md 缺失或 frontmatter 无法解析/),
    ).toBeTruthy();
    expect((await screen.findByText(reason)).textContent).toBe(reason);

    // 解析成功：整条黄条都不出现（连原话一起，守卫破了就红）
    fireEvent.click(screen.getByRole("button", { name: /alpha/ }));
    await screen.findByRole("button", { name: "发版" });
    await waitFor(() =>
      expect(
        screen.queryByText(/SKILL.md 缺失或 frontmatter 无法解析/),
      ).toBeNull(),
    );
  });

  it("落点未知时统计卡显示破折号", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    fireEvent.click(screen.getByRole("button", { name: /beta/ }));
    const detail = screen.getByRole("region", { name: "技能详情" });
    const term = await within(detail).findByText("落点");
    expect(term.nextElementSibling?.textContent).toBe("—");
  });

  it("使用 tab 一条记录一行，按作用域分组，项目用法可解除", async () => {
    showAlphaWithUsage([
      projectUsage("/p/demo"),
      globalUsage({ agents: ["universal", "dsh"], version: "1.1.0" }),
    ]);
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    fireEvent.click(await screen.findByRole("tab", { name: "使用" }));
    // 项目段：作用域是项目，跟最新时版本如实写「跟随最新」
    const projectSection = await screen.findByRole("region", {
      name: "项目使用",
    });
    expect(within(projectSection).getByText("/p/demo")).toBeTruthy();
    expect(within(projectSection).getByText("跟随最新")).toBeTruthy();
    // 全局段：没有项目，登记版本照实给出；两段都有行内解除
    const globalSection = screen.getByRole("region", { name: "全局使用" });
    expect(within(globalSection).getByText("全局")).toBeTruthy();
    expect(within(globalSection).getByText("v1.1.0")).toBeTruthy();
    expect(
      within(globalSection).getByRole("button", { name: "解除" }),
    ).toBeTruthy();
    expect(within(globalSection).queryByText(/fsh unuse/)).toBeNull();

    fireEvent.click(
      within(projectSection).getByRole("button", { name: "解除" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });
    // agent 配置里没有这个键时，勾选项退回键本身
    expect(within(dialog).getByRole("checkbox", { name: "dsh" })).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));
    // 默认全选即整条撤完：名单省缺，桥按整体撤回处理
    await waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "alpha",
        false,
        ["/p/demo"],
        undefined,
      ),
    );
  });

  it("全局段行内解除只撤那一条全局记录", async () => {
    showAlphaWithUsage([
      projectUsage("/p/one"),
      globalUsage({ agents: ["universal"] }),
    ]);
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });
    fireEvent.click(await screen.findByRole("tab", { name: "使用" }));

    const globalSection = await screen.findByRole("region", {
      name: "全局使用",
    });
    fireEvent.click(
      within(globalSection).getByRole("button", { name: "解除" }),
    );
    const dialog = await screen.findByRole("dialog", { name: "解除使用" });
    expect(within(dialog).getByText(/从 全局 撤回 alpha/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));

    await waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "alpha",
        true,
        [],
        undefined,
      ),
    );
  });

  // 「解除…」多选：页面只负责入口与重挂契约，勾选与文案在对话框直测里
  describe("解除…多选入口", () => {
    function showMixed() {
      showAlphaWithUsage([
        projectUsage("/p/one", { agents: ["dsh", "claude"] }),
        projectUsage("/p/two", { agents: ["dsh"], version: "1.0.0" }),
        globalUsage({ agents: ["dsh", "universal"] }),
      ]);
    }

    it("没有使用记录时页头入口禁用并说明", async () => {
      // beforeEach 的 alpha 详情没有使用记录
      renderPage();
      await screen.findByRole("button", { name: "使用…" });

      const entry = screen.getByRole("button", { name: "解除…" });
      expect(entry).toHaveProperty("disabled", true);
      expect(entry.getAttribute("title")).toBe("还没有使用记录");
    });

    it("关闭再打开回到默认全选", async () => {
      showMixed();
      renderPage();
      await screen.findByRole("button", { name: /alpha/ });
      fireEvent.click(await screen.findByRole("button", { name: "解除…" }));

      const dialog = await screen.findByRole("dialog", { name: "解除使用" });
      fireEvent.click(
        within(dialog).getByRole("checkbox", { name: /\/p\/one/ }),
      );
      fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));

      fireEvent.click(await screen.findByRole("button", { name: "解除…" }));
      const reopened = await screen.findByRole("dialog", { name: "解除使用" });
      const boxes = within(reopened).getAllByRole("checkbox");
      // 三条记录重新全勾上，没有继承上一次的取消
      expect(
        boxes.slice(0, 3).every((box) => (box as HTMLInputElement).checked),
      ).toBe(true);
    });
  });

  it("版本 tab 列出时间线，可对比上一版并看到文件差异", async () => {
    renderPage();
    await screen.findByRole("button", { name: /alpha/ });

    await screen.findByRole("button", { name: "发版" });
    fireEvent.click(screen.getByRole("tab", { name: "版本" }));
    expect((await screen.findAllByText("v1.1.0")).length).toBeGreaterThan(0);
    expect(screen.getByText("当前")).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", { name: /对比 v1\.0\.0 与 v1\.1\.0/ }),
    );
    await waitFor(() =>
      expect(testApi.raw.diffVersions).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        "1.1.0",
      ),
    );
    expect((await screen.findAllByText("SKILL.md")).length).toBeGreaterThan(0);
  });

  it("版本 tab 每行标出在用记录数，跟最新记录归入最新行", async () => {
    showAlphaWithUsage([
      projectUsage("/p/one"),
      projectUsage("/p/two", { version: "1.0.0" }),
      globalUsage({ agents: ["universal"] }),
    ]);
    renderPage();
    await openVersionsTab();

    const timeline = await screen.findByRole("list", { name: "版本时间线" });
    const rowFor = (version: string) =>
      within(timeline).getByText(`v${version}`).closest("li") as HTMLElement;
    // 两条跟最新的记录计入最新一行，固定版本的那条落在自己那一行
    expect(within(rowFor("1.1.0")).getByText("2 处在用")).toBeTruthy();
    expect(within(rowFor("1.0.0")).getByText("1 处在用")).toBeTruthy();
  });

  it("没有任何使用记录时切版本按钮禁用并说明原因", async () => {
    renderPage();
    await openVersionsTab();

    expect(
      await screen.findByText("还没有被任何项目或全局使用，无法切换版本"),
    ).toBeTruthy();
    const buttons = await screen.findAllByRole("button", { name: /切到 v/ });
    expect(buttons).toHaveLength(2);
    for (const button of buttons) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    }
  });

  it("移除要二次确认并列出影响面", async () => {
    renderPage();
    await screen.findByRole("heading", { name: "alpha" });

    fireEvent.click(screen.getByRole("button", { name: "移除" }));
    const dialog = await screen.findByRole("dialog", { name: "移除技能" });
    expect(within(dialog).getByText(/2 个版本 tag/)).toBeTruthy();
    expect(within(dialog).getByText(/此操作不可撤销/)).toBeTruthy();

    fireEvent.click(within(dialog).getByRole("button", { name: "移除" }));
    await waitFor(() =>
      expect(testApi.raw.removeSkill).toHaveBeenCalledWith("alpha"),
    );
  });

  it("归档先弹确认框，确认后才调用桥", async () => {
    renderPage();
    await screen.findByRole("region", { name: "技能详情" });

    fireEvent.click(await screen.findByRole("button", { name: "归档" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("归档技能")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "归档" }));
    await waitFor(() =>
      expect(testApi.raw.archiveSkill).toHaveBeenCalledWith("alpha"),
    );
  });

  it("详情头动作打开发版框", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "发版" }));
    const dialog = await screen.findByRole("dialog", { name: "发版" });
    expect(within(dialog).getByLabelText("版本号")).toBeTruthy();
  });
});
