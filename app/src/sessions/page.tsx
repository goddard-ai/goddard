import type { DaemonSession } from "@goddard-ai/sdk"
import { useState } from "preact/hooks"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { queryClient, useQuery } from "~/lib/query.ts"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsx"
import { SessionsList } from "./list.tsx"
import styles from "./page.style.ts"
import {
  filterSessionsByTitle,
  getSessionDisplayTitle,
  getSessionRepositoryLabel,
} from "./presentation.ts"

export default function SessionsPage() {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const [searchQuery, setSearchQuery] = useState("")

  const { sessions } = useQuery(goddardSdk.session.list, [{ limit: SESSION_LIST_LIMIT }])
  const visibleSessions = filterSessionsByTitle(sessions, searchQuery)

  function openSession(sessionId: DaemonSession["id"]) {
    const session = sessions.find((candidate) => candidate.id === sessionId)

    if (!session) {
      return
    }

    workbenchTabSet.openOrFocusTab({
      id: `session:${session.id}`,
      kind: "sessionChat",
      title: getSessionDisplayTitle(session),
      payload: {
        projectPath: findNearestProjectPath(projectRegistry.projectList, session.cwd),
        sessionId: session.id,
      },
      dirty: false,
    })
  }

  function openSessionChanges(sessionId: DaemonSession["id"]) {
    const session = sessions.find((candidate) => candidate.id === sessionId)

    if (!session) {
      return
    }

    queryClient.invalidate(goddardSdk.session.changes, [{ id: session.id }])
    workbenchTabSet.openOrFocusTab({
      id: `session-changes:${session.id}`,
      kind: "sessionChanges",
      title: `Changes · ${getSessionDisplayTitle(session)}`,
      payload: {
        sessionId: session.id,
        sessionTitle: getSessionDisplayTitle(session),
        repositoryLabel: getSessionRepositoryLabel(session),
      },
      dirty: false,
    })
  }

  return (
    <div class={styles.root}>
      <section class={styles.listPane}>
        <ListToolbar
          searchQuery={searchQuery}
          sessionCount={sessions.length}
          visibleSessionCount={visibleSessions.length}
          onSearchInput={(value) => {
            setSearchQuery(value)
          }}
        />
        <div class={styles.listBody}>
          <SessionsList
            onOpenChanges={openSessionChanges}
            onOpenSession={openSession}
            searchQuery={searchQuery}
            sessions={visibleSessions}
          />
        </div>
      </section>
    </div>
  )
}
