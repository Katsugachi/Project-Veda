import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    include: ["ui/tests/**/*.test.ts"],
    globals: true,
    css: false,
    testTimeout: 20000,
    hookTimeout: 20000,
    environmentOptions: {
      jsdom: { url: "http://localhost:1420/" },
    },
  },
});
