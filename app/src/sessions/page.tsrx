import type { DaemonSession } from "@goddard-ai/sdk"
import { useState } from "preact/hooks"

import { getSessionDisplayTitle, getSessionRepositoryLabel } from "./display.ts"
import { SessionsPageMutations } from "./mutations.ts"
import { useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { queryClient, useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsrx"
import { SessionsList } from "./list.tsrx"
import styles from "./page.style.ts"
import { filterSessionsByTitle } from "./title-search.ts"

component SessionsPage() {
  const workbenchTabSet = useWorkbenchTabSet()

  const { sessions } = useQuery(goddardSdk.session.list, {
    params: [{ limit: SESSION_LIST_LIMIT }],
  })

  function openSession(sessionId: DaemonSession["id"]) {
    const session = sessions.find((candidate) => candidate.id === sessionId)

    if (!session) {
      return
    }

    workbenchTabSet.openOrFocusTab({
      kind: "sessionChat",
      payload: {
        relatedFilesystemPath: session.cwd,
        sessionId: session.id,
        sessionTitle: getSessionDisplayTitle(session),
      },
    })
  }

  function openSessionChanges(sessionId: DaemonSession["id"]) {
    const session = sessions.find((candidate) => candidate.id === sessionId)

    if (!session) {
      return
    }

    queryClient.invalidate(goddardSdk.session.changes, [{ id: session.id }])
    workbenchTabSet.openOrFocusTab({
      kind: "sessionChanges",
      payload: {
        sessionId: session.id,
        sessionTitle: getSessionDisplayTitle(session),
        repositoryLabel: getSessionRepositoryLabel(session),
      },
    })
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
