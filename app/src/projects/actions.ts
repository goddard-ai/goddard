import { browseForProject } from "~/desktop-host.ts"
import type { WorkbenchTabSet } from "~/workbench-tab-set.ts"
import type { ProjectContext } from "./project-context.ts"
import { deriveProjectName } from "./project-name.ts"
import { lookupProject, type ProjectRecord, type ProjectRegistry } from "./project-registry.ts"

/** Opens one project-backed workbench tab and marks that project active. */
export function openProjectTab(props: {
  projectContext: ProjectContext
  projectPath: string
  projectRegistry: ProjectRegistry
  workbenchTabSet: WorkbenchTabSet
}) {
  const project = lookupProject(props.projectRegistry, props.projectPath)

  if (!project) {
    return null
  }

  props.projectContext.setActiveProject(project.path)
  props.workbenchTabSet.openOrFocusTab({
    id: `project:${encodeURIComponent(project.path)}`,
    kind: "project",
    title: project.name,
    payload: { projectPath: project.path },
    dirty: false,
  })
  return project
}

/** Resolves one filesystem selection into a tracked project and opens its tab. */
export async function openProjectFromFilesystem(props: {
  projectContext: ProjectContext
  projectRegistry: ProjectRegistry
  workbenchTabSet: WorkbenchTabSet
}) {
  const selectedPath = await browseForProject()

  if (!selectedPath) {
    return null
  }

  const existingProject = lookupProject(props.projectRegistry, selectedPath)
  const projectName = existingProject?.name ?? deriveProjectName(selectedPath)
  const nextProject: ProjectRecord = existingProject ?? {
    path: selectedPath,
    name: projectName.length > 0 ? projectName : selectedPath,
  }

  if (!existingProject) {
    props.projectRegistry.addProject(nextProject)
  }

  openProjectTab({
    projectContext: props.projectContext,
    projectPath: nextProject.path,
    projectRegistry: props.projectRegistry,
    workbenchTabSet: props.workbenchTabSet,
  })
  return nextProject
}
