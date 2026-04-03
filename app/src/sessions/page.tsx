import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { useSignal } from "@preact/signals"
import { MessageSquareText, Sparkles } from "lucide-react"
import { useEffect } from "preact/hooks"
import {
  useProjectRegistry,
  useSessionChat,
  useSessionIndex,
  useSessionLaunch,
  useWorkbenchTabSet,
} from "~/app-state-context.tsx"
import { List } from "./list.tsx"
import { ListToolbar } from "./list-toolbar.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"
import { lookupSession, type SessionRecord } from "./session-index.ts"

function formatTimestamp(value: number) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value))
}

export function Page() {
  const projectRegistry = useProjectRegistry()
  const sessionChat = useSessionChat()
  const sessionIndex = useSessionIndex()
  const sessionLaunch = useSessionLaunch()
  const workbenchTabSet = useWorkbenchTabSet()
  const selectedSessionId = useSignal<SessionRecord["id"] | null>(
    sessionIndex.sessionList[0]?.id ?? null,
  )
  const sessions = sessionIndex.sessionList
  const selectedSession = selectedSessionId.value
    ? lookupSession(sessionIndex, selectedSessionId.value)
    : null

  useEffect(() => {
    if (!selectedSessionId.value && sessions[0]) {
      selectedSessionId.value = sessions[0].id
    }

    if (selectedSessionId.value && !sessionIndex.getSession(selectedSessionId.value)) {
      selectedSessionId.value = sessions[0]?.id ?? null
    }
  }, [selectedSessionId, sessionIndex, sessions])

  function openSession(sessionId: SessionRecord["id"]) {
    const session = sessionIndex.getSession(sessionId)

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
        gap: "22px",
        height: "100%",
        padding: "26px",
        background:
          `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 14%, transparent), transparent 34%), ` +
          `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
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
          borderRadius: "28px",
          background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
          boxShadow: "0 28px 80px rgba(121, 138, 160, 0.12)",
        })}
      >
        <ListToolbar
          sessionCount={sessions.length}
          onCreateSession={() => {
            if (selectedSession?.cwd) {
              sessionLaunch.openDialog(selectedSession.cwd)
              return
            }

            sessionLaunch.openDialog(projectRegistry.projectList[0]?.path ?? null)
          }}
        />
        <div
          class={css({
            minHeight: "0",
            overflowY: "auto",
          })}
        >
          <List
            listStatus={sessionIndex.listStatus}
            onCreateSession={() => {
              if (selectedSession?.cwd) {
                sessionLaunch.openDialog(selectedSession.cwd)
                return
              }

              sessionLaunch.openDialog(projectRegistry.projectList[0]?.path ?? null)
            }}
            onOpenSession={openSession}
            onSelectSession={(sessionId) => {
              selectedSessionId.value = sessionId
            }}
            selectedSessionId={selectedSessionId.value}
            sessionChat={sessionChat}
            sessions={sessions}
          />
        </div>
      </section>
      <aside
        class={css({
          display: "flex",
          flexDirection: "column",
          gap: "18px",
          padding: "26px",
          border: "1px solid",
          borderColor: "border",
          borderRadius: "28px",
          background: `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.panel")} 100%)`,
          boxShadow: "0 28px 80px rgba(121, 138, 160, 0.12)",
        })}
      >
        <span
          class={css({
            display: "inline-flex",
            alignItems: "center",
            gap: "8px",
            width: "fit-content",
            padding: "8px 12px",
            borderRadius: "999px",
            backgroundColor: "surface",
            color: "accentStrong",
            fontSize: "0.72rem",
            fontWeight: "700",
            letterSpacing: "0.14em",
            textTransform: "uppercase",
          })}
        >
          <Sparkles size={14} strokeWidth={2} />
          Inspector
        </span>
        {selectedSession ? (
          <>
            <div class={css({ display: "grid", gap: "8px" })}>
              <h2
                class={css({
                  color: "text",
                  fontSize: "1.35rem",
                  fontWeight: "760",
                  letterSpacing: "-0.03em",
                  lineHeight: "1.15",
                })}
              >
                {getSessionDisplayTitle(selectedSession)}
              </h2>
              <p
                class={css({
                  color: "muted",
                  fontSize: "0.94rem",
                  lineHeight: "1.7",
                })}
              >
                {selectedSession.lastAgentMessage ?? selectedSession.cwd}
              </p>
            </div>
            <div class={css({ display: "grid", gap: "12px" })}>
              <InfoRow label="Project" value={selectedSession.cwd} />
              <InfoRow label="Created" value={formatTimestamp(selectedSession.createdAt)} />
              <InfoRow label="Updated" value={formatTimestamp(selectedSession.updatedAt)} />
              <InfoRow
                label="Transcript"
                value={`${sessionChat.messagesForSession(selectedSession.id).length} messages`}
              />
              <InfoRow label="Status" value={selectedSession.status} />
            </div>
            <button
              class={css({
                display: "inline-flex",
                alignItems: "center",
                justifyContent: "center",
                gap: "8px",
                width: "100%",
                height: "42px",
                marginTop: "auto",
                borderRadius: "14px",
                border: "1px solid",
                borderColor: "border",
                backgroundColor: "background",
                color: "text",
                fontSize: "0.9rem",
                fontWeight: "680",
                cursor: "pointer",
              })}
              type="button"
              onClick={() => {
                openSession(selectedSession.id)
              }}
            >
              <MessageSquareText size={16} strokeWidth={2.1} />
              Open chat tab
            </button>
          </>
        ) : (
          <div
            class={css({
              display: "grid",
              gap: "10px",
            })}
          >
            <h2
              class={css({
                color: "text",
                fontSize: "1.15rem",
                fontWeight: "720",
              })}
            >
              No session selected
            </h2>
            <p
              class={css({
                color: "muted",
                fontSize: "0.93rem",
                lineHeight: "1.7",
              })}
            >
              Pick a row from the list to inspect its project link, timestamps, and transcript
              count.
            </p>
          </div>
        )}
        {sessionIndex.listStatus === "error" ? (
          <p
            class={css({
              color: "danger",
              fontSize: "0.84rem",
              lineHeight: "1.6",
            })}
          >
            {sessionIndex.errorMessage}
          </p>
        ) : null}
      </aside>
    </div>
  )
}

function InfoRow(props: { label: string; value: string }) {
  return (
    <div
      class={css({
        display: "grid",
        gap: "6px",
        padding: "14px 16px",
        borderRadius: "18px",
        border: "1px solid",
        borderColor: "border",
        backgroundColor: "background",
      })}
    >
      <span
        class={css({
          color: "muted",
          fontSize: "0.78rem",
          fontWeight: "700",
          letterSpacing: "0.08em",
          textTransform: "uppercase",
        })}
      >
        {props.label}
      </span>
      <span
        class={css({
          color: "text",
          fontSize: "0.92rem",
          lineHeight: "1.5",
        })}
      >
        {props.value}
      </span>
    </div>
  )
}

export default Page
