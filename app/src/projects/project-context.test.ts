import { expect, test } from "bun:test"
import { sigma } from "preact-sigma"

import { WORKBENCH_MAIN_TAB, WorkbenchTabSet } from "~/workbench-tab-set.ts"
import {
  findNearestProjectPath,
  orderProjectsByRecentActivity,
  ProjectContext,
} from "./project-context.ts"
import { ProjectRegistry } from "./project-registry.ts"

const projects = [
  { name: "Repo", path: "/repo" },
  { name: "UI", path: "/repo/packages/ui" },
  { name: "Docs", path: "/docs" },
] as const

function replaceProjects(
  projectRegistry: ProjectRegistry,
  projectList: readonly (typeof projects)[number][],
) {
  sigma.replaceState(projectRegistry, {
    orderedProjectPaths: projectList.map((project) => project.path),
    projectsByPath: Object.fromEntries(projectList.map((project) => [project.path, project])),
  })
}

function createProjectContext() {
  const projectRegistry = new ProjectRegistry()
  const workbenchTabSet = new WorkbenchTabSet()
  const context = new ProjectContext({
    projectRegistry,
    workbenchTabSet,
  })

  return {
    context,
    projectRegistry,
    workbenchTabSet,
  }
}

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

test("contextless focused tabs keep the current active project", async () => {
  const { context, projectRegistry, workbenchTabSet } = createProjectContext()
  const cleanup = context.setup()

  try {
    replaceProjects(projectRegistry, [projects[0]])
    workbenchTabSet.openOrFocusTab({
      kind: "project",
      payload: {
        projectPath: "/repo",
      },
    })
    await Promise.resolve()
    workbenchTabSet.activateTab(WORKBENCH_MAIN_TAB.id)
    await Promise.resolve()

    expect(context.activeProjectPath).toBe("/repo")
    expect(context.recentProjectPaths).toEqual(["/repo"])
  } finally {
    cleanup()
  }
})

test("tabs without related filesystem paths keep the current active project", async () => {
  const { context, projectRegistry, workbenchTabSet } = createProjectContext()
  const cleanup = context.setup()

  try {
    replaceProjects(projectRegistry, [projects[2], projects[0]])
    context.activateProject("/docs")
    workbenchTabSet.openOrFocusTab({
      kind: "inbox",
      payload: {},
    })
    await Promise.resolve()

    expect(context.activeProjectPath).toBe("/docs")
  } finally {
    cleanup()
  }
})
