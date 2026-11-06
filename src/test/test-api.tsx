import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { type ApiMethod, apiMocks } from "@/test/api-mocks";

/**
 * 页测共享的测试基建：启用方式是在测试文件里写
 * `vi.mock("@/lib/ipc")`（走 src/lib/__mocks__/ipc.ts），
 * 再从这里取 testApi 与标准渲染器。
 */
export const testApi = {
  /** 底层 mock：断言调用、按需 mockRejectedValueOnce 都用它 */
  raw: apiMocks,
  /** 批量配置解析值：testApi.stub({ agents: [...] }) 一层覆盖 */
  stub(overrides: Partial<Record<ApiMethod, unknown>>): void {
    for (const [name, value] of Object.entries(overrides)) {
      const fn = apiMocks[name as ApiMethod];
      if (!fn) throw new Error(`testApi.stub：api.${name} 不存在`);
      fn.mockReset().mockResolvedValue(value);
    }
  },
};

/** 标准页测渲染：套 QueryClientProvider，retry 关闭让错误直达 */
export function renderWithQuery(ui: ReactNode) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    client,
    ...render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>),
  };
}
