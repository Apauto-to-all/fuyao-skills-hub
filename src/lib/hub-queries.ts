import { type QueryClient, queryOptions } from "@tanstack/react-query";
import { api, type GcReport } from "@/lib/ipc";

/**
 * 回收报告：预演结果带 executed: false，执行后写回的报告带 executed: true。
 * 界面凭这一位区分「可回收」与「已回收」。
 */
export type ReclaimReport = GcReport & { executed: boolean };

/**
 * 读侧查询的定义点：queryKey 与 queryFn 只在这里写一次，
 * 页面、命令面板与对话框一律引用，写操作后的全量失效
 * （use-hub-mutation）靠这些键的稳定性成立。
 */
export const hubQueries = {
  /** 环境快照：hub 根、初始化态、版本与平台 */
  capabilities: queryOptions({
    queryKey: ["capabilities"],
    queryFn: api.capabilities,
  }),
  /** 技能清单（含归档态） */
  skills: queryOptions({
    queryKey: ["skills"],
    queryFn: api.listSkills,
  }),
  /** agent 清单：内置 + 用户覆盖 */
  agents: queryOptions({
    queryKey: ["agents"],
    queryFn: api.agents,
  }),
  /** 项目清单：路径与每条技能的落点状态 */
  projects: queryOptions({
    queryKey: ["projects"],
    queryFn: api.projects,
  }),
  /** 环境体检报告 */
  doctor: queryOptions({
    queryKey: ["doctor"],
    queryFn: api.doctor,
  }),
  /**
   * 回收报告槽位：首次取预演（executed: false）；执行成功后执行报告
   * 写回同一槽位（executed: true），界面照旧读这一份。
   */
  gc: queryOptions({
    queryKey: ["gc"],
    queryFn: async (): Promise<ReclaimReport> => ({
      ...(await api.gc(true)),
      executed: false,
    }),
  }),
  /** 单个技能详情 */
  skill: (key: string) =>
    queryOptions({
      queryKey: ["skill", key],
      queryFn: () => api.showSkill(key),
    }),
  /** 版本差异：某技能两个版本间的文件级改动（from/to 传空串仅作禁用占位） */
  diff: (skillKey: string, from: string, to: string) =>
    queryOptions({
      queryKey: ["diff", skillKey, from, to],
      queryFn: () => api.diffVersions(skillKey, from, to),
    }),
};

/** 回收槽位的查询键：执行报告写回认这一个键 */
export const RECLAIM_QUERY_KEY = hubQueries.gc.queryKey;

/**
 * 由写操作自己填结果的查询槽位（键的第一段）：写成功后它们不参与全量失效，
 * 因为执行结果已经写进去了，重拉只会把界面打回写之前的读数。
 */
export const WRITE_FILLED_QUERY_KEYS: readonly string[] = [
  String(RECLAIM_QUERY_KEY[0]),
];

/**
 * 把一次执行结果写进回收槽位：写完之后这一槽位就是「已回收」的报告，
 * 不必再取一次预演。
 */
export function recordReclaimResult(
  queryClient: QueryClient,
  report: GcReport,
): void {
  queryClient.setQueryData(RECLAIM_QUERY_KEY, {
    ...report,
    executed: true,
  } satisfies ReclaimReport);
}
