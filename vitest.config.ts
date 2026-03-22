import { defineConfig } from "vitest/config"
import { workspaceAliases } from "./vitest.aliases"

export default defineConfig({
  test: {
    environment: "node",
    include: ["**/*.test.ts"],
    globals: true,
    silent: true,
    reporters: ["default"],
  },
  resolve: {
    alias: workspaceAliases,
    conditions: ["source", "import", "default"],
  },
})
