import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// 每个用例后清干净：主题类与本地存储会跨用例泄漏
afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.classList.remove("dark");
});

// cmdk 会观察尺寸并在选中项滚动；jsdom 没有这两个 API，给最小桩
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= ResizeObserverStub as typeof ResizeObserver;
Element.prototype.scrollIntoView ??= () => {};
