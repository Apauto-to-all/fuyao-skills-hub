import { describe, expect, it } from "vitest";
import { agentDisplayName, splitAgents } from "@/lib/agents";
import { agentRow } from "@/test/fixtures";

describe("agent 词表", () => {
  it("显示名取自配置，配置里没有这个键时退回键本身", () => {
    const agents = [agentRow()];
    expect(agentDisplayName(agents, "dsh")).toBe("DeepSeek Harness");
    expect(agentDisplayName(agents, "claude")).toBe("claude");
    expect(agentDisplayName(undefined, "dsh")).toBe("dsh");
  });

  it("名单按勾选切成要撤的与留下的，两半都跟随登记顺序", () => {
    expect(splitAgents(["universal", "dsh"], ["dsh"])).toEqual({
      withdrawn: ["dsh"],
      kept: ["universal"],
    });
    expect(splitAgents(["universal", "dsh"], [])).toEqual({
      withdrawn: [],
      kept: ["universal", "dsh"],
    });
  });
});
