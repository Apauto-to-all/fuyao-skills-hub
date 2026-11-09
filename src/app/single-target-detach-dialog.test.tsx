import { fireEvent, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SingleTargetDetachDialog } from "@/app/single-target-detach-dialog";
import type { WithdrawRequest } from "@/lib/registry";
import { agentRow } from "@/test/fixtures";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

/** 一条项目记录：两个 agent 落到同一个目录，撤哪几个只由勾选名单决定 */
const target: WithdrawRequest = {
  key: "gamma",
  agents: ["universal", "dsh"],
  targets: { global: false, projects: ["/p/three"] },
};

function renderDialog(onConfirm = vi.fn()) {
  renderWithQuery(
    <SingleTargetDetachDialog
      target={target}
      pending={false}
      onClose={() => {}}
      onConfirm={onConfirm}
    />,
  );
  return screen.findByRole("dialog", { name: "解除使用" });
}

describe("单目标解除对话框", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      agents: [
        agentRow({ key: "universal", display_name: "通用技能目录" }),
        agentRow({ key: "dsh", display_name: "DeepSeek Harness" }),
      ],
    });
  });

  it("默认全选：名单省缺，桥按整体撤回处理", async () => {
    const onConfirm = vi.fn();
    const dialog = await renderDialog(onConfirm);

    const boxes = within(dialog).getAllByRole("checkbox");
    expect(boxes.every((box) => (box as HTMLInputElement).checked)).toBe(true);
    // 点满记录时落点全删，没有留下的目录，文案不提共用目录
    expect(
      within(dialog).queryByText(/被其他 agent 共用的目录会保留/),
    ).toBeNull();

    fireEvent.click(within(dialog).getByRole("button", { name: "解除" }));
    expect(onConfirm).toHaveBeenCalledWith(undefined);
  });

  it("取消勾选一个 agent 时只提交勾上的那些", async () => {
    const onConfirm = vi.fn();
    const dialog = await renderDialog(onConfirm);

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
    expect(onConfirm).toHaveBeenCalledWith(["dsh"]);
  });

  it("全部取消勾选时确认按钮禁用并说明原因", async () => {
    const onConfirm = vi.fn();
    const dialog = await renderDialog(onConfirm);

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
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
