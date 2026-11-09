import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DetachDialog } from "@/app/library/detach-dialog";
import type { SkillUsage } from "@/lib/ipc";
import {
  agentRow,
  globalUsage,
  projectUsage,
  skillDetail,
} from "@/test/fixtures";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

/** 两条项目记录 + 一条全局记录；agent 名单刻意不同，交集才是候选 */
const mixedUsage: SkillUsage[] = [
  projectUsage("/p/one", { agents: ["dsh", "claude"] }),
  projectUsage("/p/two", { agents: ["dsh"], version: "1.0.0" }),
  globalUsage({ agents: ["dsh", "universal"] }),
];

/** 直接按数据 prop 渲染：勾选、文案与桥参数全部可测，不必搭整页 */
function renderDialog(usage: SkillUsage[] = mixedUsage) {
  renderWithQuery(
    <DetachDialog
      detail={skillDetail({ usage })}
      open
      onOpenChange={() => {}}
    />,
  );
  return screen.findByRole("dialog", { name: "解除使用" });
}

/** 勾选组里的记录勾选框（按作用域标记与目标定位） */
function recordBox(dialog: HTMLElement, label: string) {
  return within(dialog).getByRole("checkbox", { name: new RegExp(label) });
}

describe("解除多选对话框", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      // 只配 dsh 的显示名：其余键走「配置里没有就退回键本身」的退路
      agents: [agentRow()],
      detachSkill: {
        key: "alpha",
        targets: [
          { scope: "project", project: "/p/one", removed: [], actions: [] },
        ],
      },
    });
  });

  it("按全局在前、项目在后列出记录", async () => {
    const dialog = await renderDialog();

    // 记录勾选框在 agent 勾选框之前：取勾选组里的全部勾选框，首个应是全局记录
    const group = within(dialog).getByRole("group", {
      name: "要撤的使用记录",
    });
    const records = within(group).getAllByRole("checkbox");
    expect(records[0].closest("label")?.textContent).toContain("全局");
    expect(records[1].closest("label")?.textContent).toContain("/p/one");
    expect(records[2].closest("label")?.textContent).toContain("/p/two");
  });

  it("默认全选时桥收到全局开关与全部项目路径", async () => {
    const dialog = await renderDialog();

    // 默认全选：三条记录都勾上，agent 取交集（三处共有的只有 dsh）
    const confirm = within(dialog).getByRole("button", {
      name: "解除",
    }) as HTMLButtonElement;
    expect(confirm.disabled).toBe(false);
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "alpha",
        true,
        ["/p/one", "/p/two"],
        ["dsh"],
      ),
    );
  });

  it("只勾项目时全局开关关闭", async () => {
    const dialog = await renderDialog();

    fireEvent.click(recordBox(dialog, "全局"));
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));

    await waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "alpha",
        false,
        ["/p/one", "/p/two"],
        ["dsh"],
      ),
    );
  });

  it("只勾全局时项目列表为空", async () => {
    const dialog = await renderDialog();

    fireEvent.click(recordBox(dialog, "/p/one"));
    fireEvent.click(recordBox(dialog, "/p/two"));
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

  it("一条都没勾时确认禁用并说明原因", async () => {
    const dialog = await renderDialog();

    for (const label of ["/p/one", "/p/two", "全局"]) {
      fireEvent.click(recordBox(dialog, label));
    }

    expect(within(dialog).getByRole("button", { name: "解除" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(within(dialog).getByText("至少勾选一条使用记录")).toBeTruthy();
    expect(testApi.raw.detachSkill).not.toHaveBeenCalled();
  });

  it("勾中记录没有共同 agent 时确认禁用并说明逐处解除", async () => {
    const dialog = await renderDialog([
      projectUsage("/p/one", { agents: ["dsh"] }),
      globalUsage({ agents: ["universal"] }),
    ]);

    expect(within(dialog).getByRole("button", { name: "解除" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(
      within(dialog).getByText("这几处没有共同 agent，请逐处解除"),
    ).toBeTruthy();
    expect(testApi.raw.detachSkill).not.toHaveBeenCalled();
  });

  it("取消一个 agent 时桥收到剩下的", async () => {
    // 两处记录的 agent 都是这两个：交集有两个，取消一个才测得出「收到剩下的」
    const dialog = await renderDialog([
      projectUsage("/p/one", { agents: ["dsh", "claude"] }),
      globalUsage({ agents: ["dsh", "claude"] }),
    ]);

    fireEvent.click(within(dialog).getByRole("checkbox", { name: "claude" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));

    await waitFor(() =>
      expect(testApi.raw.detachSkill).toHaveBeenCalledWith(
        "alpha",
        true,
        ["/p/one"],
        ["dsh"],
      ),
    );
  });

  it("agent 全部取消时确认禁用并说明要勾一个", async () => {
    const dialog = await renderDialog([
      projectUsage("/p/one", { agents: ["dsh", "claude"] }),
      globalUsage({ agents: ["dsh", "claude"] }),
    ]);

    // 交集存在但用户逐个取消：不能拿「没有共同 agent」搪塞
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "dsh" }));
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "claude" }));

    expect(within(dialog).getByRole("button", { name: "解除" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(
      within(dialog).getByText("至少勾选一个 agent 才能解除"),
    ).toBeTruthy();
    expect(testApi.raw.detachSkill).not.toHaveBeenCalled();
  });

  it("文案说清撤哪几处、留哪几处", async () => {
    const dialog = await renderDialog();

    // 全选交集：写撤三处、另外零处留下
    expect(
      within(dialog).getByText(/撤 .*这 3 处记录上的落点与登记/),
    ).toBeTruthy();
    expect(
      within(dialog).getByText(/本技能另外 0 处记录原样留下/),
    ).toBeTruthy();
  });

  it("只撤部分 agent 时文案说明共用目录会保留", async () => {
    const dialog = await renderDialog([
      projectUsage("/p/one", { agents: ["dsh", "claude"] }),
      globalUsage({ agents: ["dsh", "claude"] }),
    ]);

    fireEvent.click(within(dialog).getByRole("checkbox", { name: "claude" }));
    expect(
      within(dialog).getByText(/被其他 agent 共用的目录会保留/),
    ).toBeTruthy();
  });
});
