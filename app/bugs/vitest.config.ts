import preact from "@preact/preset-vite"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { defineConfig } from "vitest/config"

const dirname = path.dirname(fileURLToPath(import.meta.url))

export default defineConfig({
  plugins: [preact()],
  resolve: {
    alias: {
      "~": path.resolve(dirname, "../src"),
    },
    conditions: ["bun"],
  },
  test: {
    environment: "jsdom",
    include: ["./bugs/**/*.test.ts", "./bugs/**/*.test.tsx"],
    setupFiles: ["./bugs/vitest.setup.ts"],
  },
})
