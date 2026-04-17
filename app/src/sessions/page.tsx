import type { DaemonSession } from "@goddard-ai/sdk"
import { useSignal } from "@preact/signals"
import { useEffect } from "preact/hooks"

import { useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsx"
import { AppCommand } from "~/commands/app-command.ts"
import { useQuery } from "~/lib/query.ts"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsx"
import { SessionsList } from "./list.tsx"
import styles from "./page.style.ts"
import { getSessionDisplayTitle } from "./presentation.ts"

export default function SessionsPage() {
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const selectedSessionId = useSignal<DaemonSession["id"] | null>(null)

  const { sessions } = useQuery(goddardSdk.session.list, [{ limit: SESSION_LIST_LIMIT }])
  const selectedSession =
    sessions.find((session) => session.id === selectedSessionId.value) ?? sessions[0] ?? null
  const selectedSessionUpdatedLabel = selectedSession
    ? new Intl.DateTimeFormat(undefined, {
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
      }).format(new Date(selectedSession.updatedAt))
    : null

  useEffect(() => {
    if (!selectedSessionId.value && sessions[0]) {
      selectedSessionId.value = sessions[0].id
    }

    if (
      selectedSessionId.value &&
      !sessions.some((session) => session.id === selectedSessionId.value)
    ) {
      selectedSessionId.value = sessions[0]?.id ?? null
    }
  }, [sessions, selectedSessionId])

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

  return (
    <div class={styles.root}>
      <section class={styles.listPane}>
        <ListToolbar
          sessionCount={sessions.length}
          onCreateSession={() => {
            AppCommand.navigation.openNewSessionDialog()
          }}
        />
        <div class={styles.listBody}>
          <SessionsList
            onCreateSession={() => {
              AppCommand.navigation.openNewSessionDialog()
            }}
            onOpenSession={openSession}
            onSelectSession={(sessionId) => {
              selectedSessionId.value = sessionId
            }}
            selectedSessionId={selectedSession?.id ?? null}
            sessions={sessions}
          />
        </div>
      </section>
      <aside class={styles.aside}>
        <div class={styles.intro}>
          <h2 class={styles.heading}>Session details</h2>
          <p class={styles.description}>
            {selectedSession
              ? "Selected session metadata for the current workbench."
              : "Select a session to inspect its current state."}
          </p>
        </div>
        {selectedSession ? (
          <dl class={styles.details}>
            <div class={styles.detailCard}>
              <dt class={styles.detailLabel}>Status</dt>
              <dd class={styles.detailValue}>{selectedSession.status}</dd>
            </div>
            <div class={styles.detailCard}>
              <dt class={styles.detailLabel}>Updated</dt>
              <dd class={styles.detailValue}>{selectedSessionUpdatedLabel}</dd>
            </div>
            <div class={styles.detailCard}>
              <dt class={styles.detailLabel}>Project</dt>
              <dd class={styles.detailValueWrap}>{selectedSession.cwd}</dd>
            </div>
            <div class={styles.detailCard}>
              <dt class={styles.detailLabel}>Transcript</dt>
              <dd class={styles.detailValue}>
                {selectedSession.lastAgentMessage ? "Ready" : "Waiting for first prompt"}
              </dd>
            </div>
          </dl>
        ) : null}
      </aside>
    </div>
  )
}
