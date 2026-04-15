import type { DaemonSession } from "@goddard-ai/sdk"
import { css } from "@goddard-ai/styled-system/css"
import { useSignal } from "@preact/signals"
import { useEffect } from "preact/hooks"

import { useWorkbenchTabSet } from "~/app-state-context.tsx"
import { AppCommand } from "~/commands/app-command.ts"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsx"
import { SessionsList } from "./list.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"

export default function SessionsPage() {
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
        sessionId: session.id,
      },
      dirty: false,
    })
  }

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateColumns: "minmax(0, 2fr) minmax(320px, 1fr)",
        gap: "20px",
        height: "100%",
        padding: "24px",
        "@media (max-width: 1040px)": {
          gridTemplateColumns: "1fr",
        },
      })}
    >
      <section
        class={css({
          display: "flex",
          flexDirection: "column",
          minHeight: "0",
          border: "1px solid",
          borderColor: "border",
          borderRadius: "16px",
          backgroundColor: "panel",
          overflow: "hidden",
        })}
      >
        <ListToolbar
          sessionCount={sessions.length}
          onCreateSession={() => {
            AppCommand.navigation.openNewSessionDialog()
          }}
        />
        <div
          class={css({
            minHeight: "0",
            overflowY: "auto",
          })}
        >
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
      <aside
        class={css({
          display: "grid",
          gap: "16px",
          alignContent: "start",
          padding: "20px",
          border: "1px solid",
          borderColor: "border",
          borderRadius: "16px",
          backgroundColor: "panel",
        })}
      >
        <div class={css({ display: "grid", gap: "6px" })}>
          <h2
            class={css({
              color: "text",
              fontSize: "1rem",
              fontWeight: "700",
            })}
          >
            Session details
          </h2>
          <p
            class={css({
              color: "muted",
              fontSize: "0.9rem",
              lineHeight: "1.6",
            })}
          >
            {selectedSession
              ? "Selected session metadata for the current workbench."
              : "Select a session to inspect its current state."}
          </p>
        </div>
        {selectedSession ? (
          <dl class={css({ display: "grid", gap: "12px" })}>
            <div
              class={css({
                display: "grid",
                gap: "4px",
                padding: "12px",
                borderRadius: "12px",
                border: "1px solid",
                borderColor: "border",
              })}
            >
              <dt
                class={css({
                  color: "muted",
                  fontSize: "0.78rem",
                  fontWeight: "600",
                })}
              >
                Status
              </dt>
              <dd class={css({ margin: "0", color: "text", fontWeight: "600" })}>
                {selectedSession.status}
              </dd>
            </div>
            <div
              class={css({
                display: "grid",
                gap: "4px",
                padding: "12px",
                borderRadius: "12px",
                border: "1px solid",
                borderColor: "border",
              })}
            >
              <dt
                class={css({
                  color: "muted",
                  fontSize: "0.78rem",
                  fontWeight: "600",
                })}
              >
                Updated
              </dt>
              <dd class={css({ margin: "0", color: "text", fontWeight: "600" })}>
                {selectedSessionUpdatedLabel}
              </dd>
            </div>
            <div
              class={css({
                display: "grid",
                gap: "4px",
                padding: "12px",
                borderRadius: "12px",
                border: "1px solid",
                borderColor: "border",
              })}
            >
              <dt
                class={css({
                  color: "muted",
                  fontSize: "0.78rem",
                  fontWeight: "600",
                })}
              >
                Project
              </dt>
              <dd
                class={css({
                  margin: "0",
                  color: "text",
                  fontWeight: "600",
                  wordBreak: "break-word",
                })}
              >
                {selectedSession.cwd}
              </dd>
            </div>
            <div
              class={css({
                display: "grid",
                gap: "4px",
                padding: "12px",
                borderRadius: "12px",
                border: "1px solid",
                borderColor: "border",
              })}
            >
              <dt
                class={css({
                  color: "muted",
                  fontSize: "0.78rem",
                  fontWeight: "600",
                })}
              >
                Transcript
              </dt>
              <dd class={css({ margin: "0", color: "text", fontWeight: "600" })}>
                {selectedSession.lastAgentMessage ? "Ready" : "Waiting for first prompt"}
              </dd>
            </div>
          </dl>
        ) : null}
      </aside>
    </div>
  )
}
