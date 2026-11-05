// 展示层的格式化工具：只把数值转成人话，不含业务判断。

const RELATIVE = new Intl.RelativeTimeFormat("zh-CN", { numeric: "auto" });
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** 相对时间：Unix 秒 → 「3 天前」；缺值给短横线 */
export function relativeTime(seconds: number | null): string {
  if (seconds === null) return "—";
  const delta = seconds * 1000 - Date.now();
  const abs = Math.abs(delta);
  if (abs < HOUR) return RELATIVE.format(Math.round(delta / MINUTE), "minute");
  if (abs < DAY) return RELATIVE.format(Math.round(delta / HOUR), "hour");
  if (abs < 30 * DAY) return RELATIVE.format(Math.round(delta / DAY), "day");
  if (abs < 365 * DAY)
    return RELATIVE.format(Math.round(delta / (30 * DAY)), "month");
  return RELATIVE.format(Math.round(delta / (365 * DAY)), "year");
}

/** 绝对日期：Unix 秒 → 「2026-11-05」，按本地时区取日历日 */
export function formatDate(seconds: number): string {
  const date = new Date(seconds * 1000);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}
