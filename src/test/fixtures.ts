// 页测共享夹具：技能清单、详情、使用记录、agent 与项目行都从这里取，
// 测试只覆盖自己关心的差异字段。

import type {
  AgentInfo,
  ProjectSkillView,
  ProjectView,
  SkillDetail,
  SkillSummary,
  SkillUsage,
} from "@/lib/ipc";

/** 时间基准：夹具里的更新/发版时间都相对它，避免各文件各算一份 */
export const NOW = Math.floor(Date.now() / 1000);

/** alpha 的 frontmatter：详情骨架的默认值 */
const alphaFrontmatter = {
  name: "alpha",
  description: "甲技能",
  version: "1.1.0",
  source: "owner/repo",
  tags: ["rust"],
};

/** 技能清单行骨架：默认活跃、有版本、一天前更新 */
export function skillRow(overrides: Partial<SkillSummary> = {}): SkillSummary {
  return {
    key: "alpha",
    latest_version: "1.0.0",
    archived: false,
    source: "owner/repo",
    description: "甲技能",
    tags: ["rust"],
    updated: NOW - 86_400,
    ...overrides,
  };
}

/** 技能详情骨架：默认两条版本时间线、无使用记录 */
export function skillDetail(overrides: Partial<SkillDetail> = {}): SkillDetail {
  return {
    key: "alpha",
    latest_version: "1.1.0",
    archived: false,
    frontmatter: alphaFrontmatter,
    frontmatter_error: null,
    timeline: [
      { version: "1.1.0", commit: "9f3a1c2", date: NOW - 86_400 },
      { version: "1.0.0", commit: "7b21e04", date: NOW - 3 * 86_400 },
    ],
    usage: [],
    destination_count: 1,
    content: "# alpha\n",
    ...overrides,
  };
}

/** 项目使用记录：默认单 agent dsh、跟最新 */
export function projectUsage(
  project: string,
  overrides: { agents?: string[]; version?: string | null } = {},
): SkillUsage {
  return {
    scope: "project",
    project,
    agents: overrides.agents ?? ["dsh"],
    version: overrides.version ?? null,
  };
}

/** 全局使用记录：默认单 agent dsh、跟最新 */
export function globalUsage(
  overrides: { agents?: string[]; version?: string | null } = {},
): SkillUsage {
  return {
    scope: "global",
    agents: overrides.agents ?? ["dsh"],
    version: overrides.version ?? null,
  };
}

/** agent 配置行骨架：默认 DeepSeek Harness，两个技能位都在 */
export function agentRow(overrides: Partial<AgentInfo> = {}): AgentInfo {
  return {
    key: "dsh",
    display_name: "DeepSeek Harness",
    skills_dir: ".dsh/skills",
    global_skills_dir: "~/.dsh/skills",
    source: "built_in",
    ...overrides,
  };
}

/** 项目里一条技能的登记视图：默认跟最新、junction、无落点 */
export function projectSkill(
  overrides: Partial<ProjectSkillView> = {},
): ProjectSkillView {
  return {
    key: "alpha",
    version: null,
    agents: ["dsh"],
    link: "junction",
    link_label: "目录联接（Junction）",
    destinations: [],
    ...overrides,
  };
}

/** 一个项目的全量视图：技能列表由调用方给 */
export function projectRow(
  path: string,
  skills: ProjectSkillView[],
  overrides: Partial<ProjectView> = {},
): ProjectView {
  return { path, skills, last_used: NOW, ...overrides };
}
