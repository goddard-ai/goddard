import type { Protected } from "preact-sigma"

import { browseForProject } from "~/desktop-host.ts"
import { createMutationsProvider } from "~/lib/mutations-provider.tsx"
import type { WorkbenchTabSet } from "~/workbench-tab-set.ts"
import type { ProjectContext } from "./project-context.ts"
import { deriveProjectName } from "./project-name.ts"
import type { ProjectRecord, ProjectRegistry } from "./project-registry.ts"

export const ProjectsPageMutations = createMutationsProvider<{
  openProjectTab: (projectPath: ProjectRecord["path"]) => void
  removeProject: (projectPath: ProjectRecord["path"]) => void
  selectProject: (projectPath: ProjectRecord["path"]) => void
}>("ProjectsPageMutations")

/** Opens one project-backed workbench tab and marks that project active. */
export function openProjectTab(props: {
  projectContext: Protected<ProjectContext>
  projectPath: string
  projectRegistry: Protected<ProjectRegistry>
  workbenchTabSet: Protected<WorkbenchTabSet>
}) {
  const project = props.projectRegistry.projectsByPath[props.projectPath] ?? null

  if (!project) {
    return null
  }

  props.projectContext.activateProject(project.path)
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
  projectContext: Protected<ProjectContext>
  projectRegistry: Protected<ProjectRegistry>
  workbenchTabSet: Protected<WorkbenchTabSet>
}) {
  const selectedPath = await browseForProject()

  if (!selectedPath) {
    return null
  }

  const existingProject = props.projectRegistry.projectsByPath[selectedPath] ?? null
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
