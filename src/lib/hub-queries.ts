import { queryOptions } from "@tanstack/react-query";
import { api } from "@/lib/ipc";

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
  /** 回收预演：只列将清理的项，不落盘 */
  gcPreview: queryOptions({
    queryKey: ["gc-preview"],
    queryFn: () => api.gc(true),
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
