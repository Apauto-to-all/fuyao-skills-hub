import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HealthPage } from "@/app/health";
import { Toaster } from "@/components/ui/toast";

const mocks = vi.hoisted(() => ({ doctor: vi.fn(), gc: vi.fn() }));

vi.mock("@/lib/ipc", () => ({
  api: { doctor: mocks.doctor, gc: mocks.gc },
  isGuiError: (value: unknown) =>
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value,
}));

function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <HealthPage />
      <Toaster />
    </QueryClientProvider>,
  );
}

const checks = [
  { name: "install_writable", status: "fail", detail: "安装目录不可写" },
  { name: "cloud_path", status: "warn", detail: "路径像云同步目录" },
  { name: "link_capability", status: "ok", detail: "junction 可用" },
  { name: "reclaimable", status: "ok", detail: "无大块可回收" },
];

describe("健康页", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.doctor.mockResolvedValue({ checks });
    mocks.gc.mockResolvedValue({
      items: [
        { action: "cleaned", path: "D:/hub/export/demo", bytes: 1024 },
        { action: "dirty_skipped", path: "D:/hub/export/dirty", bytes: 2048 },
      ],
      freed_bytes: 1024 * 1024,
    });
  });

  it("失败与警告分组，页头给计数", async () => {
    renderPage();
    expect(await screen.findByText("失败 1 项 · 警告 1 项")).toBeTruthy();
    expect(screen.getByText("安装目录可写")).toBeTruthy();
    expect(screen.getByText("路径像云同步目录")).toBeTruthy();
    expect(screen.getByText("通过 2 项")).toBeTruthy();
  });

  it("可回收只有可执行项时给按钮，确认后执行并替换成结果", async () => {
    renderPage();
    expect(await screen.findByText("可回收 1.0 MB")).toBeTruthy();
    expect(screen.getByText("内容被改动，已跳过")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "执行回收" }));
    const dialog = await screen.findByRole("dialog", { name: "执行回收" });
    expect(within(dialog).getByText(/将释放 1.0 MB/)).toBeTruthy();

    // 返回里混一条跳过项：释放量只取 freed_bytes，不能把脏目录字节算进去
    mocks.gc.mockResolvedValueOnce({
      items: [
        { action: "cleaned", path: "D:/hub/export/demo", bytes: 1024 },
        { action: "dirty_skipped", path: "D:/hub/export/dirty", bytes: 2048 },
      ],
      freed_bytes: 1024,
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "执行回收" }));
    await waitFor(() => expect(mocks.gc).toHaveBeenCalledWith(false));
    expect(await screen.findByText("已回收 1.0 KB")).toBeTruthy();
  });

  it("查询失败给页内错误块并可重试", async () => {
    mocks.doctor.mockRejectedValueOnce({
      code: "io",
      message: "读不到数据目录",
    });
    renderPage();
    expect(await screen.findByText("这一步没成功")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(await screen.findByText("失败 1 项 · 警告 1 项")).toBeTruthy();
  });

  it("写锁被占时给同一句 toast", async () => {
    mocks.gc
      .mockResolvedValueOnce({
        items: [{ action: "cleaned", path: "D:/hub/export/demo", bytes: 1024 }],
        freed_bytes: 1024,
      })
      .mockRejectedValueOnce({ code: "lock_held", message: "锁被占" });
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "执行回收" }));
    const dialog = await screen.findByRole("dialog", { name: "执行回收" });
    fireEvent.click(within(dialog).getByRole("button", { name: "执行回收" }));
    expect(
      await screen.findByText("另一个 fsh 进程正在写，稍后重试"),
    ).toBeTruthy();
  });

  it("没有可执行动作时不给执行按钮", async () => {
    mocks.gc.mockResolvedValue({
      items: [
        { action: "dirty_skipped", path: "D:/hub/export/dirty", bytes: 10 },
      ],
      freed_bytes: 0,
    });
    renderPage();
    expect(await screen.findByText("内容被改动，已跳过")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "执行回收" })).toBeNull();
  });
});
