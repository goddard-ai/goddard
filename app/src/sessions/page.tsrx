import type { DaemonSession } from "@goddard-ai/sdk"
import { useState } from "preact/hooks"

import { invalidateSessionChanges } from "./cache.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "./display.ts"
import { evictSessionHistory, SessionsPageMutations } from "./mutations.ts"
import { useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { getNextKeyboardRowIndex } from "~/lib/row-keyboard-navigation.ts"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsrx"
import { SessionsList } from "./list.tsrx"
import styles from "./page.style.ts"
import { filterSessionsByTitle } from "./title-search.ts"

export default function SessionsPage() @{
  const workbenchTabSet = useWorkbenchTabSet()

  const { sessions } = useQuery(goddardSdk.session.list, {
    params: [{ limit: SESSION_LIST_LIMIT }],
  })
  const [searchQuery, setSearchQuery] = useState("")
  const [activeSessionId, setActiveSessionId] = useState<DaemonSession["id"] | null>(null)
  const visibleSessions = filterSessionsByTitle(sessions, searchQuery)
  const activeSessionIndex = visibleSessions.findIndex((session) => session.id === activeSessionId)
  const resolvedActiveSession =
    activeSessionIndex >= 0 ? visibleSessions[activeSessionIndex] : visibleSessions[0] ?? null

  function openSession(sessionId: DaemonSession["id"]) {
    const session = sessions.find((candidate) => candidate.id === sessionId)

    if (!session) {
      return
    }

    evictSessionHistory(session.id)
    workbenchTabSet.openOrFocusTab({
      kind: "sessionChat",
      props: {
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

    invalidateSessionChanges(session.id)
    workbenchTabSet.openOrFocusTab({
      kind: "sessionChanges",
      props: {
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
      <ListToolbar
        {searchQuery}
        onRowNavigationKeyDown={(event) => {
          switch (event.key) {
            case "ArrowDown":
            case "ArrowUp": {
              event.preventDefault()
              const nextIndex = getNextKeyboardRowIndex({
                currentIndex: activeSessionIndex,
                direction: event.key === "ArrowDown" ? 1 : -1,
                rowCount: visibleSessions.length,
              })
              setActiveSessionId(nextIndex === null ? null : visibleSessions[nextIndex]?.id ?? null)
              return
            }
            case "Enter":
              if (resolvedActiveSession) {
                event.preventDefault()
                openSession(resolvedActiveSession.id)
              }
          }
        }}
        onSearchInput={(value) => {
          setActiveSessionId(null)
          setSearchQuery(value)
        }}
      />
      <div class={styles.listBody}>
        <SessionsList
          activeSessionId={resolvedActiveSession?.id ?? null}
          {searchQuery}
          sessions={visibleSessions}
        />
      </div>
    </div>
  </SessionsPageMutations>
}
