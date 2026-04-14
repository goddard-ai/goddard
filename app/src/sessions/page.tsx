import type { DaemonSession } from "@goddard-ai/sdk"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Sparkles } from "lucide-react"

import { useWorkbenchTabSet } from "~/app-state-context.tsx"
import { useQuery } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { SESSION_LIST_LIMIT } from "~/sessions/queries.ts"
import { ListToolbar } from "./list-toolbar.tsx"
import { SessionsList } from "./list.tsx"
import { getSessionDisplayTitle } from "./presentation.ts"

export default function SessionsPage() {
  const workbenchTabSet = useWorkbenchTabSet()

  const { sessions } = useQuery(goddardSdk.session.list, [{ limit: SESSION_LIST_LIMIT }])

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
        <ListToolbar sessionCount={sessions.length} onCreateSession={() => {}} />
        <div
          class={css({
            minHeight: "0",
            overflowY: "auto",
          })}
        >
          <SessionsList
            onCreateSession={() => {}}
            onOpenSession={openSession}
            onSelectSession={(sessionId) => {}}
            selectedSessionId={null}
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
      </aside>
    </div>
  )
}
