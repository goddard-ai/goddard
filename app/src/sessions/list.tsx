import { css } from "@goddard-ai/styled-system/css"
import { FolderPlus, Rows3 } from "lucide-react"
import type { SessionChat } from "~/session-chat/chat.ts"
import type { SessionRecord } from "./session-index.ts"
import { ListRow } from "./list-row.tsx"

export function SessionsList(props: {
  listStatus?: "idle" | "loading" | "ready" | "error"
  onCreateSession: () => void
  onOpenSession: (sessionId: SessionRecord["id"]) => void
  onSelectSession: (sessionId: SessionRecord["id"]) => void
  selectedSessionId: SessionRecord["id"] | null
  sessionChat: SessionChat
  sessions: readonly SessionRecord[]
}) {
  if (props.listStatus === "loading" && props.sessions.length === 0) {
    return (
      <div
        class={css({
          display: "grid",
          placeItems: "center",
          minHeight: "320px",
          padding: "40px",
          color: "muted",
        })}
      >
        Loading sessions...
      </div>
    )
  }

  if (props.sessions.length === 0) {
    return (
      <div
        class={css({
          display: "grid",
          placeItems: "center",
          minHeight: "320px",
          padding: "40px",
        })}
      >
        <div
          class={css({
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: "14px",
            maxWidth: "28rem",
            textAlign: "center",
          })}
        >
          <div
            class={css({
              display: "grid",
              placeItems: "center",
              width: "48px",
              height: "48px",
              borderRadius: "18px",
              backgroundColor: "surface",
              color: "accentStrong",
            })}
          >
            <Rows3 size={20} strokeWidth={1.9} />
          </div>
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
              lineHeight: "1.7",
            })}
          >
            The list and chat tab are wired. Launch one session to seed the first transcript.
          </p>
          <button
            class={css({
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              gap: "8px",
              height: "40px",
              paddingInline: "14px",
              borderRadius: "14px",
              border: "1px solid",
              borderColor: "border",
              backgroundColor: "background",
              color: "text",
              fontSize: "0.88rem",
              fontWeight: "640",
              cursor: "pointer",
            })}
            type="button"
            onClick={props.onCreateSession}
          >
            <FolderPlus size={16} strokeWidth={2.1} />
            Create the first session
          </button>
        </div>
      </div>
    )
  }

  return (
    <div
      class={css({
        display: "grid",
        gap: "14px",
        padding: "18px",
      })}
    >
      {props.sessions.map((session) => (
        <ListRow
          key={session.id}
          isSelected={session.id === props.selectedSessionId}
          lastMessage={props.sessionChat.lastMessageForSession(session.id)}
          onOpen={() => {
            props.onOpenSession(session.id)
          }}
          onSelect={() => {
            props.onSelectSession(session.id)
          }}
          session={session}
        />
      ))}
    </div>
  )
}
