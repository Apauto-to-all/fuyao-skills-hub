import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ReleaseDialog } from "@/app/library/release-dialog";
import { skillDetail } from "@/test/fixtures";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

/** 直接按数据 prop 渲染：版本判定只看详情载荷里的时间线 */
async function renderDialog() {
  renderWithQuery(
    <ReleaseDialog detail={skillDetail()} open onOpenChange={() => {}} />,
  );
  return screen.findByRole("dialog", { name: "发版" });
}

describe("发版对话框", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.raw.releaseSkill.mockImplementation(
      (_key: string, version: string) => Promise.resolve(version),
    );
  });

  it("更旧版本不能覆盖，发布键禁用", async () => {
    const dialog = await renderDialog();

    fireEvent.change(screen.getByLabelText("版本号"), {
      target: { value: "1.0.0" },
    });
    expect(within(dialog).getByText(/只能覆盖最新版 v1\.1\.0/)).toBeTruthy();
    expect(
      (
        within(dialog).getByRole("button", {
          name: "发布",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  it("目标版本已存在时先确认将覆盖", async () => {
    const dialog = await renderDialog();
    const input = screen.getByLabelText("版本号") as HTMLInputElement;
    expect(input.value).toBe("");

    fireEvent.change(input, { target: { value: "1.1.0" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "下一步" }));
    expect(within(dialog).getByText(/已存在，将覆盖/)).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "覆盖并发布" }));
    await waitFor(() =>
      expect(testApi.raw.releaseSkill).toHaveBeenCalledWith(
        "alpha",
        "1.1.0",
        true,
      ),
    );
  });
});
