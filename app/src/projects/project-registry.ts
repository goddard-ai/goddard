import { Sigma } from "preact-sigma"

/** One user-added project root stored by the app shell. */
export type ProjectRecord = {
  path: string
  name: string
}

/** Top-level public state for the machine-wide project registry. */
export type ProjectRegistryState = {
  projectsByPath: Record<string, ProjectRecord>
  orderedProjectPaths: string[]
}

/** Sigma state for the app's machine-wide project registry. */
export class ProjectRegistry extends Sigma<ProjectRegistryState> {
  constructor() {
    super({
      projectsByPath: {},
      orderedProjectPaths: [],
    })
  }

  /** Returns the projects in their current list order. */
  get projectList() {
    return this.orderedProjectPaths
      .map((projectPath) => this.projectsByPath[projectPath])
      .filter((project): project is ProjectRecord => Boolean(project))
  }

  /** Adds or updates one project in the machine-wide registry. */
  addProject(project: ProjectRecord) {
    this.projectsByPath[project.path] = project

    if (!this.orderedProjectPaths.includes(project.path)) {
      this.orderedProjectPaths.push(project.path)
    }
  }

  /** Removes one project from the machine-wide workspace scope. */
  removeProject(path: string) {
    delete this.projectsByPath[path]
    this.orderedProjectPaths = this.orderedProjectPaths.filter(
      (projectPath) => projectPath !== path,
    )
  }
}

export interface ProjectRegistry extends ProjectRegistryState {}
