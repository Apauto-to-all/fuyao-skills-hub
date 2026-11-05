import { describe, expect, it } from "vitest";
import { formatDate, relativeTime } from "@/lib/format";

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

describe("formatDate", () => {
  it("按本地日历日给出 YYYY-MM-DD", () => {
    expect(formatDate(new Date(1970, 0, 1, 12).getTime() / 1000)).toBe(
      "1970-01-01",
    );
    const noon = new Date(2026, 10, 5, 12, 0, 0).getTime() / 1000;
    expect(formatDate(noon)).toBe("2026-11-05");
  });
});
