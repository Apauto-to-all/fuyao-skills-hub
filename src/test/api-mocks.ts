import { vi } from "vitest";
import type { api } from "@/lib/ipc";

/**
 * 页测共享的 api 替身注册表：每个测试文件拿到独立一份
 * （模块注册表按文件隔离）。全部方法默认拒绝「测试未配置」，
 * 测试用 testApi.stub 按需配置返回值——漏配的方法一旦被调用，
 * 查询当场报错，用例立即失败。
 *
 * 键集由 ipc 的 api 对象派生：satisfies 要求每个方法逐项列出且值为自己的名字，
 * 桥加命令而这里漏配是编译错误。
 */
const methodNames = {
  capabilities: "capabilities",
  initHub: "initHub",
  listSkills: "listSkills",
  showSkill: "showSkill",
  diffVersions: "diffVersions",
  releaseSkill: "releaseSkill",
  archiveSkill: "archiveSkill",
  restoreSkill: "restoreSkill",
  projects: "projects",
  agents: "agents",
  attachSkill: "attachSkill",
  upgradeSkill: "upgradeSkill",
  detachSkill: "detachSkill",
  addSkill: "addSkill",
  removeSkill: "removeSkill",
  doctor: "doctor",
  gc: "gc",
  searchHub: "searchHub",
  openHubDir: "openHubDir",
} satisfies { [Name in keyof typeof api]: Name };

export type ApiMethod = keyof typeof methodNames;

export const apiMocks = Object.fromEntries(
  Object.keys(methodNames).map((name) => [
    name,
    vi.fn().mockRejectedValue(new Error(`测试未配置 api.${name} 的返回值`)),
  ]),
) as Record<ApiMethod, ReturnType<typeof vi.fn>>;
