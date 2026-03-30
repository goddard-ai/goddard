import { SigmaType } from "preact-sigma"
import { readJsonStorage, writeJsonStorage } from "../../../support/workspace-storage"

const PROJECT_REGISTRY_STORAGE_KEY = "goddard.app.projects.v1"

/** One user-added project root stored by the app shell. */
export type ProjectRecord = {
  path: string
  name: string
}

/** Persisted project registry snapshot. */
type ProjectRegistrySnapshot = {
  projects: ProjectRecord[]
}

/** Top-level public state for the machine-wide project registry. */
type ProjectRegistryShape = {
  projectsByPath: Record<string, ProjectRecord>
  orderedProjectPaths: string[]
}

/** Persists the visible projects in their current render order. */
function persistProjects(state: ProjectRegistryShape): void {
  writeJsonStorage(PROJECT_REGISTRY_STORAGE_KEY, {
    projects: state.orderedProjectPaths
      .map((projectPath) => state.projectsByPath[projectPath])
      .filter((project): project is ProjectRecord => Boolean(project)),
  })
}

/** Sigma state for the app's machine-wide project registry. */
export const ProjectRegistry = new SigmaType<ProjectRegistryShape>("ProjectRegistry")
  .defaultState({
    projectsByPath: {},
    orderedProjectPaths: [],
  })
  .computed({
    /** Returns the projects in their current list order. */
    projectList() {
      return this.orderedProjectPaths
        .map((projectPath) => this.projectsByPath[projectPath])
        .filter((project): project is ProjectRecord => Boolean(project))
    },
  })
  .actions({
    /** Loads the persisted project registry into memory. */
    loadProjects() {
      const snapshot = readJsonStorage<ProjectRegistrySnapshot>(PROJECT_REGISTRY_STORAGE_KEY, {
        projects: [],
      })

      this.projectsByPath = Object.fromEntries(
        snapshot.projects.map((project) => [project.path, project]),
      )
      this.orderedProjectPaths = snapshot.projects.map((project) => project.path)
    },

    /** Adds or updates one project in the machine-wide registry. */
    addProject(project: ProjectRecord) {
      this.projectsByPath[project.path] = project

      if (!this.orderedProjectPaths.includes(project.path)) {
        this.orderedProjectPaths.push(project.path)
      }

      persistProjects(this)
    },

    /** Removes one project from the machine-wide workspace scope. */
    removeProject(path: string) {
      delete this.projectsByPath[path]
      this.orderedProjectPaths = this.orderedProjectPaths.filter(
        (projectPath) => projectPath !== path,
      )
      persistProjects(this)
    },
  })

/** Looks up one project by its stable path. */
export function lookupProject(registry: ProjectRegistry, path: string): ProjectRecord | null {
  return registry.projectsByPath[path] ?? null
}

/** Runtime instance type for the project registry sigma state. */
export interface ProjectRegistry extends InstanceType<typeof ProjectRegistry> {}
