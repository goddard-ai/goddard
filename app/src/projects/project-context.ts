import { effect } from "@preact/signals"
import { Sigma } from "preact-sigma"
import { unique } from "radashi"

import {
  getWorkbenchTabRelatedFilesystemPath,
  WORKBENCH_MAIN_TAB,
  type WorkbenchTabSet,
} from "~/workbench-tab-set.ts"
import type { ProjectRecord, ProjectRegistry } from "./project-registry.ts"

/** Public state for the active project context and recent-project order. */
export type ProjectContextState = {
  activeProjectPath: string | null
  recentProjectPaths: string[]
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

    if (
      !matchedProject ||
      normalizedProjectPath.length > normalizeProjectPath(matchedProject.path).length
    ) {
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

    return (
      (registryIndexByPath.get(leftProject.path) ?? 0) -
      (registryIndexByPath.get(rightProject.path) ?? 0)
    )
  })
}

/** Sigma state for the app-wide active project context and recent-project order. */
export class ProjectContext extends Sigma<ProjectContextState> {
  #projectRegistry: ProjectRegistry
  #workbenchTabSet: WorkbenchTabSet
  #focusedTabId: string | null = null
  #focusedTabProjectPath: string | null = null

  constructor(input: { projectRegistry: ProjectRegistry; workbenchTabSet: WorkbenchTabSet }) {
    super({
      activeProjectPath: null,
      recentProjectPaths: [],
    })

    this.#projectRegistry = input.projectRegistry
    this.#workbenchTabSet = input.workbenchTabSet
  }

  onSetup() {
    let isDisposed = false
    let syncProjectsVersion = 0
    let focusedTabVersion = 0

    // Signal effects can run while the observed Sigma model is still committing its draft.
    // Deferring ProjectContext writes keeps cross-model updates out of that action boundary.
    return [
      () => {
        isDisposed = true
      },
      effect(() => {
        const version = ++syncProjectsVersion
        const validProjectPaths = this.#projectRegistry.projectList.map((project) => project.path)

        queueMicrotask(() => {
          if (isDisposed || version !== syncProjectsVersion) {
            return
          }

          this.syncProjects(validProjectPaths)
        })
      }),
      effect(() => {
        const version = ++focusedTabVersion
        const activeTab = this.#workbenchTabSet.activeClosableTab
        const tabId = activeTab?.id ?? WORKBENCH_MAIN_TAB.id
        const path = activeTab
          ? findNearestProjectPath(
              this.#projectRegistry.projectList,
              getWorkbenchTabRelatedFilesystemPath(activeTab),
            )
          : null

        queueMicrotask(() => {
          if (isDisposed || version !== focusedTabVersion) {
            return
          }

          if (
            this.#focusedTabId === tabId &&
            this.#focusedTabProjectPath === path &&
            (path === null || this.activeProjectPath === path)
          ) {
            return
          }

          this.#focusedTabId = tabId
          this.#focusedTabProjectPath = path

          if (path) {
            this.activateProject(path)
          }
        })
      }),
    ]
  }

  /** Removes active and recent paths that no longer exist in the registry. */
  syncProjects(validProjectPaths: readonly string[]) {
    const validProjectPathSet = new Set(validProjectPaths)
    const nextActiveProjectPath =
      this.activeProjectPath && validProjectPathSet.has(this.activeProjectPath)
        ? this.activeProjectPath
        : null
    const nextRecentProjectPaths = unique(
      this.recentProjectPaths.filter((path) => validProjectPathSet.has(path)),
    )
    const nextFocusedTabProjectPath =
      this.#focusedTabProjectPath && validProjectPathSet.has(this.#focusedTabProjectPath)
        ? this.#focusedTabProjectPath
        : null

    if (
      this.activeProjectPath === nextActiveProjectPath &&
      this.#focusedTabProjectPath === nextFocusedTabProjectPath &&
      this.recentProjectPaths.length === nextRecentProjectPaths.length &&
      this.recentProjectPaths.every((path, index) => path === nextRecentProjectPaths[index])
    ) {
      return
    }

    this.activeProjectPath = nextActiveProjectPath
    this.recentProjectPaths = nextRecentProjectPaths
    this.#focusedTabProjectPath = nextFocusedTabProjectPath
  }

  /** Activates one project and moves it to the front of recent-project order. */
  activateProject(path: string | null) {
    if (this.activeProjectPath === path) {
      return
    }

    this.activeProjectPath = path

    if (path) {
      this.recentProjectPaths = [path, ...this.recentProjectPaths.filter((item) => item !== path)]
    }
  }

  /** Removes one project path from active and recent project-context state. */
  removeProject(path: string) {
    const nextRecentProjectPaths = this.recentProjectPaths.filter((item) => item !== path)
    const focusedTabProjectPath =
      this.#focusedTabProjectPath === path ? null : this.#focusedTabProjectPath

    this.recentProjectPaths = nextRecentProjectPaths
    this.#focusedTabProjectPath = focusedTabProjectPath

    if (this.activeProjectPath !== path) {
      return
    }

    if (focusedTabProjectPath) {
      this.activateProject(focusedTabProjectPath)
      return
    }

    this.activeProjectPath = nextRecentProjectPaths[0] ?? null
  }
}

export interface ProjectContext extends ProjectContextState {}
