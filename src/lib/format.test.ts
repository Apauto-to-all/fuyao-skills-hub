import { describe, expect, it } from "vitest";
import { relativeTime } from "@/lib/format";

describe("relativeTime", () => {
  it("缺值给短横线", () => {
    expect(relativeTime(null)).toBe("—");
  });

  it("按量级换单位", () => {
    const now = Math.floor(Date.now() / 1000);
    expect(relativeTime(now - 2 * 3600)).toMatch(/小时前/);
    expect(relativeTime(now - 3 * 86_400)).toBe("3天前");
  });
});
