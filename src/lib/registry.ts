// 登记簿的纯规则：使用记录怎么读、怎么认行、怎么折成一次写操作的目标集合。
// 本模块不依赖 React，也不碰桥——词表、行键、分组与折叠只有这一份住所。

import type { Scope, SkillUsage } from "./ipc";

/** 全局目标的人读名：作用域说法与单目标确认文案共用 */
export const GLOBAL_TARGET_LABEL = "全局";

/** 作用域说法：项目 / 全局 */
export function scopeLabel(scope: Scope): string {
  return scope === "project" ? "项目" : GLOBAL_TARGET_LABEL;
}

/** 版本说法：跟最新时如实写「跟随最新」，不冒充某个 tag */
export function usageVersionLabel(version: string | null): string {
  return version === null ? "跟随最新" : `v${version}`;
}

/** 一行记录的目标：项目记录写路径，全局记录写「全局」 */
export function usageTargetLabel(usage: SkillUsage): string {
  return usage.scope === "project" ? usage.project : GLOBAL_TARGET_LABEL;
}

/** 版本切换的标题：切到某一版的说法只在这里拼 */
export function upgradeToLabel(version: string): string {
  return `切换到 v${version}`;
}

/** 按 agent 撤回时留下的那一句：谁留下、共用目录为什么保留 */
export function keptAgentsNote(keptNames: string): string {
  return `${keptNames}的落点与登记原样留下。被其他 agent 共用的目录会保留。`;
}

/**
 * 按作用域分组：全局段在前、项目段在后，与使用 tab 的展示顺序一致；
 * 段内保持传入顺序。
 */
export function groupByScope(usage: readonly SkillUsage[]): {
  global: SkillUsage[];
  project: SkillUsage[];
} {
  return {
    global: usage.filter((record) => record.scope === "global"),
    project: usage.filter((record) => record.scope === "project"),
  };
}

/** 行身份：作用域 + 目标 + 登记版本 + agent 名单，同一记录不会重复出行 */
export function usageRowKey(usage: SkillUsage): string {
  const target = usage.scope === "project" ? usage.project : "";
  return `${usage.scope}:${target}:${usage.version ?? "latest"}:${usage.agents.join(",")}`;
}

/** 若干条记录 agent 名单的交集：core 要的是一份名单且必须是每个目标的子集 */
export function intersectAgents(records: readonly SkillUsage[]): string[] {
  if (records.length === 0) return [];
  return records
    .slice(1)
    .reduce<string[]>(
      (kept, record) => kept.filter((agent) => record.agents.includes(agent)),
      [...records[0].agents],
    );
}

/** 一条记录的目标：作用域 + 项目路径（全局目标没有项目） */
type UsageTarget = { scope: "project"; project: string } | { scope: "global" };

/** 一组目标折出的集合：全局开关 + 项目路径列表 */
export type UsageTargets = {
  global: boolean;
  projects: string[];
};

/**
 * 把一组目标折成桥要的集合：任一目标落在全局就打开全局开关，
 * 项目路径按传入顺序收齐（登记簿里同一技能在同一项目只有一条记录，
 * 因此不会出现重复路径）。
 */
export function usageTargets(targets: readonly UsageTarget[]): UsageTargets {
  let global = false;
  const projects: string[] = [];
  for (const target of targets) {
    if (target.scope === "global") global = true;
    else projects.push(target.project);
  }
  return { global, projects };
}

/** 一次解除的目标：技能键、这条记录登记的 agent 名单与目标集合 */
export type WithdrawRequest = {
  /** 技能唯一键 */
  key: string;
  /** 这条记录登记的 agent 名单 */
  agents: string[];
  /** 要撤的目标集合 */
  targets: UsageTargets;
};
