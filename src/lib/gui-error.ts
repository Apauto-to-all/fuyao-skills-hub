/** 失败形状：code 是稳定错误码，message 是可直接展示的中文消息 */
export type GuiError = { code: string; message: string };

/**
 * 形状守卫：Tauri 的 reject 可能是字符串或 Error，
 * 只有带 code/message 的才算桥错误。纯函数、无桥依赖，
 * 被测代码与测试替身共享同一份实现。
 */
export function isGuiError(value: unknown): value is GuiError {
  if (typeof value !== "object" || value === null) return false;
  const maybe = value as { code?: unknown; message?: unknown };
  return typeof maybe.code === "string" && typeof maybe.message === "string";
}
