import type { DaemonSession } from "@goddard-ai/sdk"
import { useEffect, useState } from "preact/hooks"
import { useSearchNavigation } from "@goddard-ai/ui-primitives"

import { invalidateSessionChanges } from "./cache.ts"
import { getSessionDisplayTitle, getSessionRepositoryLabel } from "./display.ts"
import { evictSessionHistory, SessionsPageMutations } from "./mutations.ts"
import { useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsrx"
import { SessionsList } from "./list.tsrx"
import styles from "./page.style.ts"
import { filterSessionsByTitle } from "./title-search.ts"
import { filterPrimarySessions } from "./visibility.ts"

export default function SessionsPage(props: { projectName?: string; projectPath?: string }) @{
  const workbenchTabSet = useWorkbenchTabSet()

  const { sessions } = useQuery(goddardSdk.session.list, {
    params: [{ limit: SESSION_LIST_LIMIT }],
  })
  const [searchQuery, setSearchQuery] = useState("")
  const primarySessions = filterPrimarySessions(sessions)
  const visibleSessions = filterSessionsByTitle(primarySessions, searchQuery)
  const visibleSessionIds = visibleSessions.map((session) => session.id).join("\n")

  const navigation = useSearchNavigation({
    activeAttribute: "data-active",
    count: () => visibleSessions.length,
    onActivate(index) {
      const session = visibleSessions[index]

      if (session) {
        openSession(session.id)
      }
    },
    onQueryChange(query) {
      setSearchQuery(query)
    },
  })

  useEffect(() => {
    navigation.resetActiveIndex()
  }, [navigation, visibleSessionIds])

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
      <ListToolbar inputRef={navigation.inputRef} {searchQuery} />
      <div class={styles.listBody}>
        <SessionsList
          itemRef={navigation.itemRef}
          {searchQuery}
          sessions={visibleSessions}
          showProjectNames={!props.projectPath}
        />
      </div>
    </div>
  </SessionsPageMutations>
}
