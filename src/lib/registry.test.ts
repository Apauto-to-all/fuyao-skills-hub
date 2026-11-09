import { describe, expect, it } from "vitest";
import type { SkillUsage } from "@/lib/ipc";
import {
  GLOBAL_TARGET_LABEL,
  groupByScope,
  intersectAgents,
  scopeLabel,
  upgradeToLabel,
  usageRowKey,
  usageTargetLabel,
  usageTargets,
  usageVersionLabel,
} from "@/lib/registry";
import { globalUsage, projectUsage } from "@/test/fixtures";

// 三条记录覆盖两种作用域：两条项目（一条跟最新、一条固定版本）与一条全局，
// agent 名单刻意不同，交集与分组断言才有区分度
const projectOne: SkillUsage = {
  scope: "project",
  project: "/p/one",
  agents: ["dsh", "claude"],
  version: null,
};
const projectTwo: SkillUsage = {
  scope: "project",
  project: "/p/two",
  agents: ["dsh"],
  version: "1.0.0",
};
const globalOne: SkillUsage = {
  scope: "global",
  agents: ["dsh", "universal"],
  version: null,
};

describe("登记簿词表", () => {
  it("版本说法：跟最新时如实写「跟随最新」，有版本号就写 v 前缀", () => {
    expect(usageVersionLabel(null)).toBe("跟随最新");
    expect(usageVersionLabel("1.2.0")).toBe("v1.2.0");
  });

  it("目标说法：项目记录写路径，全局记录写「全局」", () => {
    expect(usageTargetLabel(projectUsage("/p/one"))).toBe("/p/one");
    expect(usageTargetLabel(globalUsage())).toBe(GLOBAL_TARGET_LABEL);
  });

  it("作用域说法只有项目与全局两种", () => {
    expect(scopeLabel("project")).toBe("项目");
    expect(scopeLabel("global")).toBe(GLOBAL_TARGET_LABEL);
  });

  it("版本切换的标题带上目标版本", () => {
    expect(upgradeToLabel("1.0.0")).toBe("切换到 v1.0.0");
  });
});

describe("行键", () => {
  it("区分作用域、目标、登记版本与 agent 名单", () => {
    expect(usageRowKey(projectOne)).toBe("project:/p/one:latest:dsh,claude");
    expect(usageRowKey(projectTwo)).toBe("project:/p/two:1.0.0:dsh");
    expect(usageRowKey(globalOne)).toBe("global::latest:dsh,universal");
  });
});

describe("分作用域", () => {
  it("全局段在前、项目段在后，段内保持原顺序", () => {
    const groups = groupByScope([projectOne, globalOne, projectTwo]);
    expect(groups.global).toEqual([globalOne]);
    expect(groups.project).toEqual([projectOne, projectTwo]);
  });

  it("没有记录时两段都是空列表", () => {
    expect(groupByScope([])).toEqual({ global: [], project: [] });
  });
});

describe("agent 交集", () => {
  it("没有记录就没有候选", () => {
    expect(intersectAgents([])).toEqual([]);
  });

  it("只有一条记录时候选就是它的名单", () => {
    expect(intersectAgents([globalOne])).toEqual(["dsh", "universal"]);
  });

  it("多条记录取共有的名单，顺序跟随首条记录", () => {
    expect(intersectAgents([projectOne, globalOne])).toEqual(["dsh"]);
    expect(intersectAgents([globalOne, projectOne])).toEqual(["dsh"]);
  });

  it("没有共同 agent 时为空列表", () => {
    const alone: SkillUsage = {
      scope: "global",
      agents: ["universal"],
      version: null,
    };
    expect(intersectAgents([projectOne, alone])).toEqual([]);
  });
});

describe("目标集合折叠", () => {
  it("任一记录落在全局就把全局开关打开，项目路径按记录顺序收齐", () => {
    expect(usageTargets([projectOne, globalOne, projectTwo])).toEqual({
      global: true,
      projects: ["/p/one", "/p/two"],
    });
  });

  it("只选中项目记录时全局开关关闭", () => {
    expect(usageTargets([projectOne, projectTwo])).toEqual({
      global: false,
      projects: ["/p/one", "/p/two"],
    });
  });

  it("只选中全局记录时项目列表为空", () => {
    expect(usageTargets([globalOne])).toEqual({
      global: true,
      projects: [],
    });
  });

  it("没有目标时开关关闭且列表为空", () => {
    expect(usageTargets([])).toEqual({ global: false, projects: [] });
  });
});
