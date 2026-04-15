import { SigmaType } from "preact-sigma"

import type { ProjectRecord } from "./project-registry.ts"
import { readJsonStorage, writeJsonStorage } from "~/support/workspace-storage.ts"

const PROJECT_CONTEXT_STORAGE_KEY = "goddard.app.project-context.v1"

type ProjectContextSnapshot = {
  activeProjectPath: string | null
  recentProjectPaths: string[]
}

type ProjectContextShape = {
  activeProjectPath: string | null
  focusedTabId: string | null
  recentProjectPaths: string[]
  reportedTabProjectsByTabId: Record<string, string | null>
}

function persistProjectContext(state: {
  activeProjectPath: string | null
  recentProjectPaths: readonly string[]
}) {
  writeJsonStorage(PROJECT_CONTEXT_STORAGE_KEY, {
    activeProjectPath: state.activeProjectPath,
    recentProjectPaths: state.recentProjectPaths,
  })
}

function normalizeProjectPath(path: string) {
  const normalizedPath = path.trim().replace(/\\/g, "/").replace(/\/+/g, "/")

  if (normalizedPath === "/") {
    return normalizedPath
  }

  if (/^[A-Za-z]:\/$/.test(normalizedPath)) {
    return normalizedPath
  }

  return normalizedPath.replace(/\/+$/, "")
}

function isContainingProjectPath(containerPath: string, candidatePath: string) {
  if (containerPath === candidatePath) {
    return true
  }

  if (containerPath === "/") {
    return candidatePath.startsWith("/")
  }

  if (/^[A-Za-z]:\/$/.test(containerPath)) {
    return candidatePath.toLowerCase().startsWith(containerPath.toLowerCase())
  }

  return candidatePath.startsWith(`${containerPath}/`)
}

function uniqueProjectPaths(paths: readonly string[]) {
  return [...new Set(paths)]
}

function setActiveProjectState(state: ProjectContextShape, path: string | null) {
  if (state.activeProjectPath === path) {
    return
  }

  state.activeProjectPath = path

  if (path) {
    state.recentProjectPaths = [path, ...state.recentProjectPaths.filter((item) => item !== path)]
  }

  persistProjectContext(state)
}

/** Resolves one filesystem path to the nearest containing opened project path. */
export function findNearestProjectPath(
  projects: readonly ProjectRecord[],
  candidatePath: string | null | undefined,
) {
  if (!candidatePath) {
    return null
  }

  const normalizedCandidatePath = normalizeProjectPath(candidatePath)
  let matchedProject: ProjectRecord | null = null

  for (const project of projects) {
    const normalizedProjectPath = normalizeProjectPath(project.path)

    if (!isContainingProjectPath(normalizedProjectPath, normalizedCandidatePath)) {
      continue
    }

    if (!matchedProject || normalizedProjectPath.length > normalizeProjectPath(matchedProject.path).length) {
      matchedProject = project
    }
  }

  return matchedProject?.path ?? null
}

/** Returns projects sorted by recent active-project order, preserving registry order otherwise. */
export function orderProjectsByRecentActivity(
  projects: readonly ProjectRecord[],
  recentProjectPaths: readonly string[],
) {
  const recentIndexByPath = new Map(recentProjectPaths.map((path, index) => [path, index]))
  const registryIndexByPath = new Map(projects.map((project, index) => [project.path, index]))

  return [...projects].sort((leftProject, rightProject) => {
    const leftRecentIndex = recentIndexByPath.get(leftProject.path)
    const rightRecentIndex = recentIndexByPath.get(rightProject.path)

    if (leftRecentIndex !== undefined && rightRecentIndex !== undefined) {
      return leftRecentIndex - rightRecentIndex
    }

    if (leftRecentIndex !== undefined) {
      return -1
    }

    if (rightRecentIndex !== undefined) {
      return 1
    }

    return (registryIndexByPath.get(leftProject.path) ?? 0) - (registryIndexByPath.get(rightProject.path) ?? 0)
  })
}

/** Sigma state for the app-wide active project context and recent-project order. */
export const ProjectContext = new SigmaType<ProjectContextShape>("ProjectContext")
  .defaultState({
    activeProjectPath: null,
    focusedTabId: null,
    recentProjectPaths: [],
    reportedTabProjectsByTabId: {},
  })
  .actions({
    /** Loads persisted project-context state and prunes paths that no longer exist. */
    hydrate(validProjectPaths: readonly string[]) {
      const snapshot = readJsonStorage<ProjectContextSnapshot>(PROJECT_CONTEXT_STORAGE_KEY, {
        activeProjectPath: null,
        recentProjectPaths: [],
      })
      const focusedTabProjectPath =
        this.focusedTabId === null ? null : this.reportedTabProjectsByTabId[this.focusedTabId] ?? null

      this.activeProjectPath = focusedTabProjectPath ?? snapshot.activeProjectPath
      this.recentProjectPaths = focusedTabProjectPath
        ? [focusedTabProjectPath, ...snapshot.recentProjectPaths.filter((path) => path !== focusedTabProjectPath)]
        : snapshot.recentProjectPaths
      this.syncProjects(validProjectPaths)
    },

    /** Removes active, recent, and reported paths that no longer exist in the registry. */
    syncProjects(validProjectPaths: readonly string[]) {
      const validProjectPathSet = new Set(validProjectPaths)
      const nextActiveProjectPath =
        this.activeProjectPath && validProjectPathSet.has(this.activeProjectPath)
          ? this.activeProjectPath
          : null
      const nextRecentProjectPaths = uniqueProjectPaths(
        this.recentProjectPaths.filter((path) => validProjectPathSet.has(path)),
      )
      const nextReportedTabProjectsByTabId = Object.fromEntries(
        Object.entries(this.reportedTabProjectsByTabId).filter((entry) => {
          const reportedPath = entry[1]
          return reportedPath === null || validProjectPathSet.has(reportedPath)
        }),
      )

      if (
        this.activeProjectPath === nextActiveProjectPath &&
        this.recentProjectPaths.length === nextRecentProjectPaths.length &&
        this.recentProjectPaths.every((path, index) => path === nextRecentProjectPaths[index]) &&
        Object.keys(this.reportedTabProjectsByTabId).length ===
          Object.keys(nextReportedTabProjectsByTabId).length &&
        Object.entries(this.reportedTabProjectsByTabId).every(
          ([tabId, path]) => nextReportedTabProjectsByTabId[tabId] === path,
        )
      ) {
        return
      }

      this.activeProjectPath = nextActiveProjectPath
      this.recentProjectPaths = nextRecentProjectPaths
      this.reportedTabProjectsByTabId = nextReportedTabProjectsByTabId
      persistProjectContext(this)
    },

    /** Marks one project as active and moves it to the front of recent-project order. */
    setActiveProject(path: string | null) {
      setActiveProjectState(this, path)
    },

    /** Applies the currently focused tab and any synchronous project resolution it already knows. */
    applyFocusedTabProject(tabId: string, path: string | null) {
      if (
        this.focusedTabId === tabId &&
        this.reportedTabProjectsByTabId[tabId] === path &&
        (path === null || this.activeProjectPath === path)
      ) {
        return
      }

      this.focusedTabId = tabId
      this.reportedTabProjectsByTabId[tabId] = path

      if (path) {
        setActiveProjectState(this, path)
      }
    },

    /** Reports one asynchronously resolved project path for a tab that may still be focused. */
    reportTabProject(tabId: string, path: string | null) {
      if (
        this.reportedTabProjectsByTabId[tabId] === path &&
        (this.focusedTabId !== tabId || path === null || this.activeProjectPath === path)
      ) {
        return
      }

      this.reportedTabProjectsByTabId[tabId] = path

      if (this.focusedTabId === tabId && path) {
        setActiveProjectState(this, path)
      }
    },

    /** Clears one tab-scoped reported project after the tab stops being active. */
    clearTabProject(tabId: string) {
      delete this.reportedTabProjectsByTabId[tabId]
    },

    /** Removes one project path from active, recent, and reported project-context state. */
    removeProject(path: string) {
      const nextRecentProjectPaths = this.recentProjectPaths.filter((item) => item !== path)
      const nextReportedTabProjectsByTabId = Object.fromEntries(
        Object.entries(this.reportedTabProjectsByTabId).filter((entry) => entry[1] !== path),
      )
      const focusedTabProjectPath =
        this.focusedTabId === null ? null : nextReportedTabProjectsByTabId[this.focusedTabId] ?? null

      this.recentProjectPaths = nextRecentProjectPaths
      this.reportedTabProjectsByTabId = nextReportedTabProjectsByTabId

      if (this.activeProjectPath !== path) {
        persistProjectContext(this)
        return
      }

      if (focusedTabProjectPath) {
        setActiveProjectState(this, focusedTabProjectPath)
        return
      }

      this.activeProjectPath = nextRecentProjectPaths[0] ?? null
      persistProjectContext(this)
    },
  })

/** Runtime instance type for the shared project-context state. */
export interface ProjectContext extends InstanceType<typeof ProjectContext> {}
