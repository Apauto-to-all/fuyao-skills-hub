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
  /** 半截装配时补齐的缺件（仅修复出现） */
  repaired?: string[];
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

/** 时间线的一项：版本号、发布提交短哈希与提交时间（Unix 秒） */
export type VersionEntry = {
  version: string;
  commit: string;
  date: number;
};

/** 单个文件的 unified diff */
export type DiffFile = {
  path: string;
  patch: string;
};

/** 技能详情：概览统计与 SKILL.md 原文 */
export type SkillDetail = {
  key: string;
  latest_version: string | null;
  archived: boolean;
  frontmatter: Frontmatter | null;
  timeline: VersionEntry[];
  referenced_by: string[];
  /** 未知（agent 配置读不出来）时为 null，UI 显示破折号而非零 */
  destination_count: number | null;
  content: string | null;
};

/** 链接方式：符号链接 / junction / 纯拷贝 */
export type LinkKind = "symlink" | "junction" | "copy";

/** 一处落点：可能被多个 agent 共用 */
export type DestinationView = {
  path: string;
  agents: string[];
  exists: boolean;
};

/** 项目里一条技能的登记视图 */
export type ProjectSkillView = {
  key: string;
  version: string | null;
  agents: string[];
  link: LinkKind;
  destinations: DestinationView[];
};

/** 一个项目的全量视图 */
export type ProjectView = {
  path: string;
  last_used: number;
  skills: ProjectSkillView[];
};

/** agent 的一处使用记录 */
export type AgentUsage = {
  project: string;
  key: string;
  version: string | null;
};

/** agent 清单行：配置 + 反查的使用记录 */
export type AgentInfo = {
  key: string;
  display_name: string;
  skills_dir: string;
  source: "built_in" | "user";
  usages: AgentUsage[];
};

/** 入库结果：键与首版号 */
export type AddReport = {
  key: string;
  version: string;
};

/** 使用结果：登记项与刚建立的落点 */
export type UseReport = {
  key: string;
  version: string | null;
  project: string;
  agents: string[];
  link: LinkKind;
  destinations: { path: string; agents: string[] }[];
  cleanup: { removed: number; freed_bytes: number } | null;
};

/** 解除结果：删掉的落点路径 */
export type UnuseReport = {
  key: string;
  project: string;
  removed: string[];
};

/** 自检结论三态 */
export type CheckStatus = "ok" | "warn" | "fail";

/** 单条自检结论 */
export type DoctorCheck = {
  name: string;
  status: CheckStatus;
  detail: string;
};

/** 八项自检报告 */
export type DoctorReport = { checks: DoctorCheck[] };

/** 一条回收动作：dry-run 表示将发生，执行后表示已发生 */
export type GcAction =
  | { action: "cleaned"; path: string; bytes: number }
  | { action: "orphan_cleaned"; path: string; bytes: number }
  | { action: "dirty_skipped"; path: string; bytes: number }
  | { action: "dead_record_removed"; path: string }
  | { action: "grace_kept"; path: string };

/** 回收报告：动作清单与释放（或可释放）字节 */
export type GcReport = {
  items: GcAction[];
  freed_bytes: number;
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
  diffVersions: (key: string, from: string, to: string) =>
    call<DiffFile[]>("diff_versions", { key, from, to }),
  releaseSkill: (key: string, version: string, overwrite: boolean) =>
    call<string>("release_skill", { key, version, overwrite }),
  archiveSkill: (key: string) => call<void>("archive_skill", { key }),
  restoreSkill: (key: string) => call<void>("restore_skill", { key }),
  projects: () => call<ProjectView[]>("projects"),
  agents: () => call<AgentInfo[]>("agents"),
  attachSkill: (
    key: string,
    version: string | null,
    project: string,
    agents: string[],
  ) => call<UseReport>("use_skill", { key, version, project, agents }),
  detachSkill: (key: string, project: string) =>
    call<UnuseReport>("unuse_skill", { key, project }),
  addSkill: (source: string, prefix?: string) =>
    call<AddReport>("add_skill", { source, prefix }),
  removeSkill: (key: string) => call<void>("remove_skill", { key }),
  doctor: () => call<DoctorReport>("doctor"),
  gc: (dryRun: boolean) => call<GcReport>("gc", { dry_run: dryRun }),
  openHubDir: () => call<void>("open_hub_dir"),
};
