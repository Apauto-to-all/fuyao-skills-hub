// 前端与 Rust 桥的唯一出口：命令名、参数与失败形状都在这里收口。

import { invoke } from "@tauri-apps/api/core";

/** 失败形状：code 是稳定错误码，message 是可直接展示的中文消息 */
export type GuiError = { code: string; message: string };

/** 环境快照：hub 根、是否已初始化、版本与平台 */
export type Capabilities = {
  hub_root: string;
  initialized: boolean;
  app_version: string;
  platform: string;
};

/** 初始化结果：字段与 core 的 InitOutcome 同名 */
export type InitReport = {
  initialized?: boolean;
  already_initialized?: boolean;
  actions?: string[];
};

/** SKILL.md frontmatter 五字段（version / source 可缺省） */
export type Frontmatter = {
  name: string;
  description: string;
  version: string | null;
  source: string | null;
  tags: string[];
};

/** 库清单行：列表只看这些字段 */
export type SkillSummary = {
  key: string;
  latest_version: string | null;
  archived: boolean;
  source: string | null;
  description: string | null;
  tags: string[];
  /** 最新版本的发布提交时间（Unix 秒） */
  updated: number | null;
};

/** 技能详情：概览统计与 SKILL.md 原文 */
export type SkillDetail = {
  key: string;
  latest_version: string | null;
  archived: boolean;
  frontmatter: Frontmatter | null;
  versions: string[];
  referenced_by: string[];
  /** 未知（agent 配置读不出来）时为 null，UI 显示破折号而非零 */
  destination_count: number | null;
  content: string | null;
};

/** 形状守卫：Tauri 的 reject 可能是字符串或 Error，只有带 code/message 的才算桥错误 */
export function isGuiError(value: unknown): value is GuiError {
  if (typeof value !== "object" || value === null) return false;
  const maybe = value as { code?: unknown; message?: unknown };
  return typeof maybe.code === "string" && typeof maybe.message === "string";
}

/** 所有命令的统一入口：非桥错误兜成 unknown，页面只需处理一种形状 */
export async function call<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (error) {
    if (isGuiError(error)) throw error;
    throw { code: "unknown", message: String(error) } satisfies GuiError;
  }
}

export const api = {
  capabilities: () => call<Capabilities>("capabilities"),
  initHub: () => call<InitReport>("init_hub"),
  listSkills: () => call<SkillSummary[]>("list_skills"),
  showSkill: (key: string) => call<SkillDetail>("show_skill", { key }),
  openHubDir: () => call<void>("open_hub_dir"),
};
