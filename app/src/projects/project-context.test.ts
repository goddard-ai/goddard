import { expect, test } from "bun:test"

import {
  findNearestProjectPath,
  orderProjectsByRecentActivity,
  ProjectContext,
} from "./project-context.ts"

const projects = [
  { name: "Repo", path: "/repo" },
  { name: "UI", path: "/repo/packages/ui" },
  { name: "Docs", path: "/docs" },
] as const

test("findNearestProjectPath prefers the nearest containing opened project", () => {
  expect(findNearestProjectPath(projects, "/repo/packages/ui/src")).toBe("/repo/packages/ui")
  expect(findNearestProjectPath(projects, "/repo/packages/api")).toBe("/repo")
  expect(findNearestProjectPath(projects, "/outside")).toBeNull()
})

test("orderProjectsByRecentActivity prioritizes recent projects and preserves registry order otherwise", () => {
  expect(orderProjectsByRecentActivity(projects, ["/docs", "/repo/packages/ui"])).toEqual([
    projects[2],
    projects[1],
    projects[0],
  ])
})

test("contextless focused tabs keep the current active project", () => {
  const context = new ProjectContext()

  context.activateProject("/repo")
  context.applyFocusedTabProject("main", null)

  expect(context.activeProjectPath).toBe("/repo")
  expect(context.recentProjectPaths).toEqual(["/repo"])
})

test("late async tab reports do not override the active project after focus has moved", () => {
  const context = new ProjectContext()

  context.activateProject("/docs")
  context.applyFocusedTabProject("session:1", null)
  context.applyFocusedTabProject("main", null)
  context.reportTabProject("session:1", "/repo")

  expect(context.activeProjectPath).toBe("/docs")
})
