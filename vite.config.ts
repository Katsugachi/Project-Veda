import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  root: "ui",
  clearScreen: false,
  server: {
    host: host || "0.0.0.0",
    port: 1420,
    strictPort: true,
    allowedHosts: true,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: "esbuild",
    sourcemap: true,
    outDir: "../dist",
    emptyOutDir: true,
  },
});
