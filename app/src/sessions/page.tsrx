import type { DaemonSession } from "@goddard-ai/sdk"
import { useState } from "preact/hooks"

import { SessionsPageMutations } from "./actions.ts"
import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { queryClient, useQuery } from "~/lib/query.ts"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsrx"
import { SessionsList } from "./list.tsrx"
import styles from "./page.style.ts"
import {
  filterSessionsByTitle,
  getSessionDisplayTitle,
  getSessionRepositoryLabel,
} from "./presentation.ts"

component SessionsPage() {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()

  const { sessions } = useQuery(goddardSdk.session.list, [
    { limit: SESSION_LIST_LIMIT },
  ])

  function openSession(sessionId: DaemonSession["id"]) {
    const session = sessions.find((candidate) => candidate.id === sessionId)

    if (!session) {
      return
    }

    workbenchTabSet.openOrFocusTab(
      {
        id: `session:${session.id}`,
        kind: "sessionChat",
        title: getSessionDisplayTitle(session),
        payload: {
          projectPath: findNearestProjectPath(projectRegistry.projectList, session.cwd),
          sessionId: session.id,
        },
        dirty: false,
      },
    )
  }

  function openSessionChanges(sessionId: DaemonSession["id"]) {
    const session = sessions.find((candidate) => candidate.id === sessionId)

    if (!session) {
      return
    }

    queryClient.invalidate(goddardSdk.session.changes, [{ id: session.id }])
    workbenchTabSet.openOrFocusTab(
      {
        id: `session-changes:${session.id}`,
        kind: "sessionChanges",
        title: `Changes · ${getSessionDisplayTitle(session)}`,
        payload: {
          sessionId: session.id,
          sessionTitle: getSessionDisplayTitle(session),
          repositoryLabel: getSessionRepositoryLabel(session),
        },
        dirty: false,
      },
    )
  }

  <SessionsPageMutations
    mutations={{
      openSession,
      openSessionChanges,
    }}
  >
    <div class={styles.root}>
      const [searchQuery, setSearchQuery] = useState("")

      <ListToolbar
        {searchQuery}
        onSearchInput={(value) => {
          setSearchQuery(value)
        }}
      />
      <div class={styles.listBody}>
        const visibleSessions = filterSessionsByTitle(sessions, searchQuery)

        <SessionsList {searchQuery} sessions={visibleSessions} />
      </div>
    </div>
  </SessionsPageMutations>
}

export default SessionsPage
