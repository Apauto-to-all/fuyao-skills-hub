import { vi } from "vitest";

/**
 * 页测共享的 api 替身注册表：每个测试文件拿到独立一份
 * （模块注册表按文件隔离）。全部方法默认拒绝「测试未配置」，
 * 测试用 testApi.stub 按需配置返回值——漏配的方法一旦被调用，
 * 查询当场报错，用例立即失败。
 */
const methods = [
  "capabilities",
  "initHub",
  "listSkills",
  "showSkill",
  "diffVersions",
  "releaseSkill",
  "archiveSkill",
  "restoreSkill",
  "projects",
  "agents",
  "attachSkill",
  "upgradeSkill",
  "detachSkill",
  "addSkill",
  "removeSkill",
  "doctor",
  "gc",
  "openHubDir",
] as const;

export type ApiMethod = (typeof methods)[number];

export const apiMocks: Record<
  ApiMethod,
  ReturnType<typeof vi.fn>
> = {} as Record<ApiMethod, ReturnType<typeof vi.fn>>;

for (const name of methods) {
  apiMocks[name] = vi
    .fn()
    .mockRejectedValue(new Error(`测试未配置 api.${name} 的返回值`));
}
