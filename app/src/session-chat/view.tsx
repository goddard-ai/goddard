import type { DaemonSession } from "@goddard-ai/sdk"
import { css } from "@goddard-ai/styled-system/css"
import { token } from "@goddard-ai/styled-system/tokens"
import { Composer } from "./composer.tsx"
import { buildTranscriptMessages } from "./chat.ts"
import { Header } from "./header.tsx"
import { Transcript } from "./transcript.tsx"
import { useQueries, useQueryClient } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import {
  getSession,
  getSessionHistory,
  listSessions,
  SESSION_LIST_LIMIT,
} from "~/sessions/queries.ts"

export function SessionChatView(props: { sessionId: string }) {
  const queryClient = useQueryClient()
  const sessionId = props.sessionId as DaemonSession["id"]
  const [{ history, session }] = useQueries({
    history: [getSessionHistory, [sessionId]],
    session: [getSession, [sessionId]],
  })
  const messages = buildTranscriptMessages(session, history.history)

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateRows: "auto minmax(0, 1fr) auto",
        height: "100%",
        background:
          `radial-gradient(circle at top right, color-mix(in srgb, ${token.var("colors.accent")} 12%, transparent), transparent 28%), ` +
          `linear-gradient(180deg, ${token.var("colors.background")} 0%, ${token.var("colors.surface")} 100%)`,
      })}
    >
      <Header messageCount={messages.length} session={session} />
      <Transcript
        initialScrollPosition="bottom"
        messages={messages}
        scrollCacheKey={`detail:session:${session.id}:transcript`}
      />
      <Composer
        onSubmit={async (text) => {
          try {
            await goddardSdk.session.prompt({
              id: session.id,
              acpId: session.acpSessionId,
              prompt: text,
            })
            queryClient.invalidate(listSessions, [SESSION_LIST_LIMIT])
            queryClient.invalidate(getSession, [session.id])
            queryClient.invalidate(getSessionHistory, [session.id])
          } catch (error) {
            console.error("Failed to submit session prompt.", error)
          }
        }}
      />
    </div>
  )
}

export default SessionChatView
