import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UseDialog } from "@/app/library/use-dialog";
import { agentRow, skillDetail } from "@/test/fixtures";
import { renderWithQuery, testApi } from "@/test/test-api";

vi.mock("@/lib/ipc");

// 目录选择器替换成可控桩，不启动 Tauri
const dialog = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialog.open }));

/** 直接按数据 prop 渲染：表单只依赖详情载荷与 agent 清单 */
async function renderDialog() {
  renderWithQuery(
    <UseDialog detail={skillDetail()} open onOpenChange={() => {}} />,
  );
  return screen.findByRole("dialog", { name: /使用 alpha/ });
}

describe("使用对话框", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    testApi.stub({
      agents: [agentRow()],
      attachSkill: {
        key: "alpha",
        version: null,
        targets: [
          {
            scope: "project",
            project: "D:/work/alpha",
            agents: ["dsh"],
            link: "junction",
            destinations: [],
            actions: [],
          },
        ],
        cleanup: null,
      },
    });
    dialog.open.mockResolvedValue("D:/work/alpha");
  });

  it("选目录与 agent 后提交", async () => {
    const form = await renderDialog();

    fireEvent.click(within(form).getByRole("button", { name: "选择…" }));
    expect(await within(form).findByText("D:/work/alpha")).toBeTruthy();

    fireEvent.click(
      await within(form).findByRole("checkbox", { name: /DeepSeek/ }),
    );
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));
    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        null,
        false,
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });

  it("只勾全局时不带项目目录", async () => {
    const form = await renderDialog();

    fireEvent.click(
      within(form).getByRole("checkbox", { name: /装到全局技能目录/ }),
    );
    // 勾上全局后 agent 行改显全局位，避免选到没有全局位的 agent
    expect(await within(form).findByText("~/.dsh/skills")).toBeTruthy();
    fireEvent.click(
      await within(form).findByRole("checkbox", { name: /DeepSeek/ }),
    );
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
  });

  it("两者都选时项目与全局一起提交", async () => {
    const form = await renderDialog();

    fireEvent.click(within(form).getByRole("button", { name: "选择…" }));
    await within(form).findByText("D:/work/alpha");
    fireEvent.click(
      within(form).getByRole("checkbox", { name: /装到全局技能目录/ }),
    );
    fireEvent.click(
      await within(form).findByRole("checkbox", { name: /DeepSeek/ }),
    );
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));
    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        null,
        true,
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });

  it("一个目标都不给时不可提交并说明原因", async () => {
    const form = await renderDialog();

    fireEvent.click(
      await within(form).findByRole("checkbox", { name: /DeepSeek/ }),
    );

    expect(within(form).getByText(/至少选一个目标/)).toBeTruthy();
    expect(
      (within(form).getByRole("button", { name: "接入" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("可选固定版本", async () => {
    const form = await renderDialog();

    fireEvent.click(within(form).getByRole("button", { name: "选择…" }));
    await within(form).findByText("D:/work/alpha");
    fireEvent.change(within(form).getByLabelText("版本"), {
      target: { value: "1.0.0" },
    });
    fireEvent.click(
      await within(form).findByRole("checkbox", { name: /DeepSeek/ }),
    );
    fireEvent.click(within(form).getByRole("button", { name: "接入" }));
    await waitFor(() =>
      expect(testApi.raw.attachSkill).toHaveBeenCalledWith(
        "alpha",
        "1.0.0",
        false,
        "D:/work/alpha",
        ["dsh"],
      ),
    );
  });
});
