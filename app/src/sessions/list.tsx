import type { DaemonSession } from "@goddard-ai/sdk"
import { css } from "@goddard-ai/styled-system/css"
import { Plus } from "lucide-react"

import { ListRow } from "./list-row.tsx"

export function SessionsList(props: {
  errorMessage?: string | null
  listStatus?: "idle" | "loading" | "ready" | "error"
  onCreateSession: () => void
  onOpenSession: (sessionId: DaemonSession["id"]) => void
  onSelectSession: (sessionId: DaemonSession["id"]) => void
  selectedSessionId: DaemonSession["id"] | null
  sessions: readonly DaemonSession[]
}) {
  if (props.listStatus === "loading" && props.sessions.length === 0) {
    return (
      <div
        class={css({
          display: "grid",
          alignContent: "start",
          minHeight: "320px",
          padding: "20px",
          color: "muted",
        })}
      >
        Loading sessions...
      </div>
    )
  }

  if (props.listStatus === "error" && props.sessions.length === 0) {
    return (
      <div
        class={css({
          display: "grid",
          alignContent: "start",
          minHeight: "320px",
          padding: "20px",
          color: "muted",
        })}
      >
        <div class={css({ display: "grid", gap: "10px", maxWidth: "28rem" })}>
          <h2
            class={css({
              color: "text",
              fontSize: "1.15rem",
              fontWeight: "720",
            })}
          >
            Couldn&apos;t load sessions
          </h2>
          <p
            class={css({
              lineHeight: "1.7",
            })}
          >
            {props.errorMessage ?? "The daemon-backed session list request failed."}
          </p>
        </div>
      </div>
    )
  }

  if (props.sessions.length === 0) {
    return (
      <div
        class={css({
          display: "grid",
          alignContent: "start",
          gap: "12px",
          minHeight: "220px",
          padding: "20px",
        })}
      >
        <div class={css({ display: "grid", gap: "8px", maxWidth: "28rem" })}>
          <h2
            class={css({
              color: "text",
              fontSize: "1.15rem",
              fontWeight: "720",
            })}
          >
            No sessions yet
          </h2>
          <p
            class={css({
              color: "muted",
              fontSize: "0.93rem",
              lineHeight: "1.6",
            })}
          >
            Launch one session to seed the first transcript.
          </p>
          <div>
            <button
              class={css({
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                gap: "8px",
                height: "36px",
                paddingInline: "12px",
                borderRadius: "10px",
                border: "1px solid",
                borderColor: "accent",
                backgroundColor: "surface",
                color: "text",
                fontSize: "0.86rem",
                fontWeight: "600",
                cursor: "pointer",
              })}
              type="button"
              onClick={props.onCreateSession}
            >
              <Plus size={16} strokeWidth={2.1} />
              New session
            </button>
          </div>
        </div>
      </div>
    )
  }

  return (
    <ul
      class={css({
        listStyle: "none",
        margin: "0",
        padding: "0",
        "& > li + li": {
          borderTop: "1px solid",
          borderColor: "border",
        },
      })}
    >
      {props.sessions.map((session) => (
        <li key={session.id}>
          <ListRow
            isSelected={session.id === props.selectedSessionId}
            onOpen={() => {
              props.onOpenSession(session.id)
            }}
            onSelect={() => {
              props.onSelectSession(session.id)
            }}
            session={session}
          />
        </li>
      ))}
    </ul>
  )
}
