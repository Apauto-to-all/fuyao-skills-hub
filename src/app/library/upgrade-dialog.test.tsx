import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UpgradeDialog } from "@/app/library/upgrade-dialog";
import type { SkillUsage } from "@/lib/ipc";
import { globalUsage, projectUsage, skillDetail } from "@/test/fixtures";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

/** 三条使用记录：两条跟最新、一条固定 v1.0.0，全局与项目各一处 */
const threeUsages: SkillUsage[] = [
  projectUsage("/p/one"),
  projectUsage("/p/two", { version: "1.0.0" }),
  globalUsage({ agents: ["universal"] }),
];

/** 直接按数据 prop 渲染：勾选状态与桥参数都可测 */
function renderDialog(usage: SkillUsage[] = threeUsages) {
  renderWithQuery(
    <UpgradeDialog
      detail={skillDetail({ usage })}
      version="1.0.0"
      open
      onOpenChange={() => {}}
    />,
  );
  return screen.findByRole("dialog", { name: "切换到 v1.0.0" });
}

describe("版本切换对话框", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      upgradeSkill: {
        key: "alpha",
        version: "1.0.0",
        targets: [],
        cleanup: null,
      },
    });
  });

  it("默认全选，每行写清作用域、目标与当前版本", async () => {
    const dialog = await renderDialog();

    const boxes = within(dialog).getAllByRole("checkbox") as HTMLInputElement[];
    expect(boxes).toHaveLength(3);
    expect(boxes.every((box) => box.checked)).toBe(true);
    expect(within(dialog).getAllByText("项目")).toHaveLength(2);
    expect(within(dialog).getByText("/p/two")).toBeTruthy();
    expect(within(dialog).getByText("全局")).toBeTruthy();
    expect(within(dialog).getAllByText("跟随最新")).toHaveLength(2);
  });

  it("去掉一条记录后提交剩下的项目与全局", async () => {
    const dialog = await renderDialog();

    fireEvent.click(within(dialog).getByRole("checkbox", { name: /\/p\/two/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "切换" }));

    await waitFor(() =>
      expect(testApi.raw.upgradeSkill).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        true,
        ["/p/one"],
      ),
    );
  });

  it("一条都不勾时确认禁用并说明原因", async () => {
    const dialog = await renderDialog();

    for (const box of within(dialog).getAllByRole("checkbox")) {
      fireEvent.click(box);
    }

    expect(within(dialog).getByText("至少勾选一条使用记录")).toBeTruthy();
    expect(
      (
        within(dialog).getByRole("button", {
          name: "切换",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(testApi.raw.upgradeSkill).not.toHaveBeenCalled();
  });
});
