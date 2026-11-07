// 前端与 Rust 桥的唯一出口：命令名、参数与失败形状都在这里收口。
// 错误守卫 isGuiError 与 GuiError 形状定义在 gui-error 模块，这里随桥一并出口。

import { invoke } from "@tauri-apps/api/core";
import { type GuiError, isGuiError } from "./gui-error";

export type { GuiError };
export { isGuiError };

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
  /** 全局装着此技能的 agent 键；没有全局用法时后端不写这一项 */
  global_agents?: string[];
  /** 未知（agent 配置读不出来）时为 null，UI 显示破折号而非零 */
  destination_count: number | null;
  content: string | null;
};

/** 链接方式的机器值；显示名由后端 link_label 一并给出 */
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
  /** 链接方式的人读名（中文打头、括号带英文原名），直接显示 */
  link_label: string;
  destinations: DestinationView[];
};

/** 一个项目的全量视图 */
export type ProjectView = {
  path: string;
  last_used: number;
  skills: ProjectSkillView[];
};

/** 用法的作用域：铺给某个项目，或铺给全局技能目录 */
export type Scope = "project" | "global";

/** agent 的一处使用记录 */
export type AgentUsage = {
  scope: Scope;
  /** 项目根绝对路径；全局用法没有项目 */
  project?: string;
  key: string;
  version: string | null;
};

/** agent 清单行：配置 + 反查的使用记录 */
export type AgentInfo = {
  key: string;
  display_name: string;
  /** 项目相对技能目录；只有全局位的 agent 没有 */
  skills_dir?: string;
  /** 展开后的全局技能目录绝对路径；只有项目位的 agent 没有 */
  global_skills_dir?: string;
  source: "built_in" | "user";
  usages: AgentUsage[];
};

/** 入库结果：键与首版号 */
export type AddReport = {
  key: string;
  version: string;
};

/** 一个目标的使用结果：作用域、落点与链接方式 */
export type TargetReport = {
  scope: Scope;
  /** 项目根绝对路径；全局目标没有项目 */
  project?: string;
  agents: string[];
  /** 实际采用的链接方式；预演没有这一项 */
  link?: LinkKind;
  destinations: { path: string; agents: string[] }[];
  /** 预演时的动作清单；实跑为空 */
  actions: string[];
};

/** 使用结果：逐目标的登记项与刚建立的落点 */
export type UseReport = {
  key: string;
  version: string | null;
  targets: TargetReport[];
  cleanup: { removed: number; freed_bytes: number } | null;
};

/** 一个目标的撤回结果：作用域、登记 agent 与删掉的落点 */
export type UnuseTargetReport = {
  scope: Scope;
  /** 项目根绝对路径；全局目标没有项目 */
  project?: string;
  /** 本次撤走的 agent；预演没有这一项 */
  agents?: string[];
  /** 该记录登记的链接方式；预演没有这一项 */
  link?: LinkKind;
  /** 实际删除的落点；预演为空 */
  removed: string[];
  /** 预演时的动作清单；实跑为空 */
  actions: string[];
};

/** 解除结果：逐目标的撤回结果 */
export type UnuseReport = {
  key: string;
  targets: UnuseTargetReport[];
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
    global: boolean,
    project: string | null,
    agents: string[],
  ) => call<UseReport>("use_skill", { key, version, global, project, agents }),
  detachSkill: (key: string, project: string, agents: string[]) =>
    call<UnuseReport>("unuse_skill", { key, project, agents }),
  addSkill: (source: string, prefix?: string) =>
    call<AddReport>("add_skill", { source, prefix }),
  removeSkill: (key: string) => call<void>("remove_skill", { key }),
  doctor: () => call<DoctorReport>("doctor"),
  gc: (dryRun: boolean) => call<GcReport>("gc", { dry_run: dryRun }),
  openHubDir: () => call<void>("open_hub_dir"),
};
