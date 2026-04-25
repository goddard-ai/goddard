import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { findNearestProjectPath } from "./project-context.ts"

/** Resolves an arbitrary filesystem path to the nearest user-added project. */
export function useNearestProjectPath(path: string | null | undefined) {
  const projectRegistry = useProjectRegistry()

  return findNearestProjectPath(projectRegistry.projectList, path)
}

/** Reports the active workbench tab's implied project while its surface is mounted. */
export function useReportTabProject(projectPath: string | null | undefined) {
  const projectContext = useProjectContext()
  const workbenchTabSet = useWorkbenchTabSet()
  const tabId = workbenchTabSet.activeTabId
  const reportedProjectPath = projectPath ?? null

  useEffect(() => {
    projectContext.reportTabProject(tabId, reportedProjectPath)

    return () => {
      projectContext.clearTabProject(tabId)
    }
  }, [projectContext, reportedProjectPath, tabId])
}
