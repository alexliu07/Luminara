import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// 剥掉 jassub 里指向缺失源码的 sourcemap 注释，避免 Vite 每次都警告
const stripJassubSourcemaps = () => ({
  name: "strip-jassub-sourcemaps",
  enforce: "pre",
  transform(code, id) {
    if (id.includes("jassub") && code.includes("sourceMappingURL")) {
      return {
        code: code.replace(/\n?\/\/# sourceMappingURL=[^\n]*/g, ""),
        map: null,
      };
    }
  },
});

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [
    stripJassubSourcemaps(),
    vue({
      template: {
        compilerOptions: {
          // Video.js 10 的自定义元素
          isCustomElement: (tag) => tag === "video-player" || tag === "video-skin",
        },
      },
    }),
  ],

  // jassub 的 worker 由 JASSUB 以 type:'module' 加载，且内部用 import.meta.url，必须走 ES 格式
  worker: {
    format: "es",
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 5173,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
