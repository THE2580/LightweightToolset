import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
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
      // 3. tell Vite to ignore watching `src-tauri`
      //    另外忽略非前端目录：agent_memory 由编辑工具原子写入（会短暂出现 .tmpdir 目录，
      //    被 Vite 的 watcher watch 到会 EBUSY 直接打崩 dev server）。
      ignored: ["**/src-tauri/**", "**/agent_memory/**", "**/docs/**", "**/.codex/**", "**/.*.tmpdir/**"],
    },
  },
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        popup: "popup.html",
        free: "free.html",
        notification: "notification.html",
      },
    },
  },
}));
