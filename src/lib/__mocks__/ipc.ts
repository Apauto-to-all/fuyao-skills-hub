// "@/lib/ipc" 的测试替身：api 换成共享注册表，isGuiError 用真实现。
// 页测文件里一行 vi.mock("@/lib/ipc") 显式启用。

import { apiMocks } from "../../test/api-mocks";
import { isGuiError } from "../gui-error";

export const api = apiMocks;
export type { GuiError } from "../gui-error";
export { isGuiError };
