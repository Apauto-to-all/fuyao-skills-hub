import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import { call, isGuiError } from "@/lib/ipc";

// invoke 是 Tauri 桥的唯一外部依赖：测试里换成可控桩
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("isGuiError", () => {
  it("带字符串 code 与 message 的对象是桥错误", () => {
    expect(isGuiError({ code: "io", message: "读不到" })).toBe(true);
  });

  it("字符串、null、Error、缺字段的对象都不是", () => {
    expect(isGuiError("磁盘没了")).toBe(false);
    expect(isGuiError(null)).toBe(false);
    expect(isGuiError(new Error("boom"))).toBe(false);
    expect(isGuiError({ code: "io" })).toBe(false);
    expect(isGuiError({ code: 1, message: "x" })).toBe(false);
  });
});

describe("call", () => {
  it("成功时原样传命令与参数并返回数据", async () => {
    vi.mocked(invoke).mockResolvedValue(42);
    await expect(call("capabilities", { x: 1 })).resolves.toBe(42);
    expect(invoke).toHaveBeenCalledWith("capabilities", { x: 1 });
  });

  it("桥错误原样抛出，不二次包装", async () => {
    const bridge = { code: "lock_held", message: "锁被占" };
    vi.mocked(invoke).mockRejectedValue(bridge);
    await expect(call("gc")).rejects.toBe(bridge);
  });

  it("未知错误兜成 unknown 信封：字符串", async () => {
    vi.mocked(invoke).mockRejectedValue("磁盘没了");
    await expect(call("gc")).rejects.toEqual({
      code: "unknown",
      message: "磁盘没了",
    });
  });

  it("未知错误兜成 unknown 信封：Error 实例", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("boom"));
    await expect(call("gc")).rejects.toEqual({
      code: "unknown",
      message: "Error: boom",
    });
  });
});
