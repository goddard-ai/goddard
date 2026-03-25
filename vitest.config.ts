import { defineConfig } from "vitest/config"
import { workspaceAliases } from "./vitest.aliases"

export default defineConfig({
  test: {
    environment: "node",
    include: ["**/*.test.ts"],
    globals: true,
    silent: "passed-only",
    reporters: ["dot"],
    passWithNoTests: true,
  },
  resolve: {
    alias: workspaceAliases,
  },
})
