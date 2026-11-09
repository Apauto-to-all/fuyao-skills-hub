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

/**
 * 一条使用记录：一条登记记录一行。
 * 作用域决定形状——项目记录必有项目路径，全局记录没有项目
 */
export type SkillUsage =
  | {
      scope: "project";
      /** 项目根路径（展示形态，可直接显示与传回桥） */
      project: string;
      /** 这条记录登记的 agent 键 */
      agents: string[];
      /** 固定版本；跟最新时为空 */
      version: string | null;
    }
  | {
      scope: "global";
      agents: string[];
      version: string | null;
    };

/** 技能详情：概览统计与 SKILL.md 原文 */
export type SkillDetail = {
  key: string;
  latest_version: string | null;
  archived: boolean;
  frontmatter: Frontmatter | null;
  /** frontmatter 解析失败的原因（点名文件与字段）；解析成功或读不到 SKILL.md 时为 null */
  frontmatter_error: string | null;
  timeline: VersionEntry[];
  /** 使用记录：项目段在前、全局段在后，一条记录一行 */
  usage: SkillUsage[];
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

/** agent 清单行：只有配置本身 */
export type AgentInfo = {
  key: string;
  display_name: string;
  /** 项目相对技能目录；只有全局位的 agent 没有 */
  skills_dir?: string;
  /** 配置里写的全局技能目录原文；只有项目位的 agent 没有 */
  global_skills_dir?: string;
  source: "built_in" | "user";
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

/** 一个目标的版本切换结果：换了哪条记录、切换前的版本与换后的落点 */
export type UpgradeTargetReport = TargetReport & {
  /** 切换前登记的版本；原来就跟随最新时为空 */
  from: string | null;
  /** 这条记录本来就在目标版本：零盘动作 */
  idempotent: boolean;
  /** 幂等命中的提醒（拷贝档跟最新时盘上仍是旧内容） */
  note?: string;
};

/** 版本切换结果：逐目标的切换结果与顺手回收的统计 */
export type UpgradeReport = {
  key: string;
  version: string | null;
  targets: UpgradeTargetReport[];
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

/** 发版结果：实跑回键与落章版本；预演字段只在预演出现，桥只实跑 */
export type ReleaseReport = {
  key: string;
  version: string;
  dry_run?: boolean;
  actions?: string[];
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

/** 检索命中行：行号、行文本与行内字符偏移命中区间 */
export type SearchHit = {
  /** 技能唯一键；store 外命中省略 */
  key?: string;
  /** 相对 hub 根路径（正斜杠） */
  path: string;
  /** 行号（1 起） */
  line: number;
  /** 行文本 */
  text: string;
  /** 命中片段 [start, end)，按字符偏移 */
  spans: [number, number][];
  /** 上下文行（请求了才回） */
  context?: {
    before: { line: number; text: string }[];
    after: { line: number; text: string }[];
  };
};

/** 检索结果：命中（恒定排序）+ 精确总数与截断标记 */
export type SearchResult = {
  hits: SearchHit[];
  total: number;
  truncated: boolean;
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
    call<ReleaseReport>("release_skill", { key, version, overwrite }).then(
      (r) => r.version,
    ),
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
  upgradeSkill: (
    key: string,
    version: string,
    global: boolean,
    projects: string[],
  ) => call<UpgradeReport>("upgrade_skill", { key, version, global, projects }),
  detachSkill: (
    key: string,
    global: boolean,
    projects: string[],
    agents?: string[],
  ) => call<UnuseReport>("unuse_skill", { key, global, projects, agents }),
  addSkill: (source: string, prefix?: string) =>
    call<AddReport>("add_skill", { source, prefix }),
  removeSkill: (key: string) => call<void>("remove_skill", { key }),
  doctor: () => call<DoctorReport>("doctor"),
  gc: (dryRun: boolean) => call<GcReport>("gc", { dry_run: dryRun }),
  searchHub: (query: string, context?: number) =>
    call<SearchResult>("search_hub", { query, context }),
  openHubDir: () => call<void>("open_hub_dir"),
};
