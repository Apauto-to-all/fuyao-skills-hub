import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
// @ts-expect-error 未安装 @types/node 时此导入会报类型错误
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// Vite 配置说明见 https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react()],

  // 专为 Tauri 开发调整的 Vite 选项，仅在 `tauri dev` 或 `tauri build` 时生效
  //
  // 1. 防止 Vite 清屏遮蔽 Rust 错误信息
  clearScreen: false,
  // 2. Tauri 要求固定端口，端口不可用时直接失败
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. 让 Vite 忽略对 `src-tauri` 的监听
      ignored: ["**/src-tauri/**"],
    },
  },
}));
