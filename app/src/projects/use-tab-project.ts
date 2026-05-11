import { createContext } from "preact"
import { useContext, useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry } from "~/app-state-context.tsrx"
import { findNearestProjectPath } from "./project-context.ts"

/** Contextual workbench tab id for hooks rendered inside one tab panel. */
export const ReportedProjectTabIdContext = createContext<string | null>(null)

/** Resolves an arbitrary filesystem path to the nearest user-added project. */
export function useNearestProjectPath(path: string | null | undefined) {
  const projectRegistry = useProjectRegistry()

  return findNearestProjectPath(projectRegistry.projectList, path)
}

/** Reports the contextual workbench tab's implied project while its surface is mounted. */
export function useReportTabProject(projectPath: string | null | undefined) {
  const projectContext = useProjectContext()
  const tabId = useContext(ReportedProjectTabIdContext)
  const reportedProjectPath = projectPath ?? null

  useEffect(() => {
    if (tabId === null) {
      return
    }

    projectContext.reportTabProject(tabId, reportedProjectPath)
  }, [projectContext, reportedProjectPath, tabId])

  useEffect(() => {
    if (tabId === null) {
      return
    }

    return () => {
      projectContext.clearTabProject(tabId)
    }
  }, [projectContext, tabId])
}
