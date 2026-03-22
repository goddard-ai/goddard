import { defineConfig } from "vitest/config"
import { workspaceProjects } from "./vitest.workspace.ts"

export default defineConfig({
  test: {
    projects: workspaceProjects,
  },
})
