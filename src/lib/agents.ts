import type { AgentInfo } from "./ipc";

/**
 * agent 词表：勾选组与确认文案共用的显示名解析与名单切分。
 * 配置里没有这个键（用户删了覆盖）时退回键本身。
 */
export function agentDisplayName(
  agents: readonly AgentInfo[] | undefined,
  key: string,
): string {
  return agents?.find((agent) => agent.key === key)?.display_name ?? key;
}

/**
 * 一条记录上的 agent 名单按勾选切成两半：要撤的与留下的。
 * 顺序都跟随登记名单，切分只做筛选，不改顺序。
 */
export function splitAgents(
  registered: readonly string[],
  checked: readonly string[],
): { withdrawn: string[]; kept: string[] } {
  return {
    withdrawn: registered.filter((key) => checked.includes(key)),
    kept: registered.filter((key) => !checked.includes(key)),
  };
}
