import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// 每个用例后清干净：主题类与本地存储会跨用例泄漏
afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.classList.remove("dark");
});
