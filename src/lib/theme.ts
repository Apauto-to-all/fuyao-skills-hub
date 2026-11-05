// 主题的唯一读写处：类名给 CSS 用，localStorage 给首屏内联脚本用。

export type Theme = "light" | "dark";

const STORAGE_KEY = "fsh-theme";

/** 读当前主题：根元素带 dark 类即为暗色 */
export function readTheme(): Theme {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

/** 应用并持久化主题：存储键 fsh-theme 由本模块独占 */
export function applyTheme(theme: Theme): void {
  document.documentElement.classList.toggle("dark", theme === "dark");
  localStorage.setItem(STORAGE_KEY, theme);
}
