import type { DaemonSession } from "@goddard-ai/sdk"
import { css } from "@goddard-ai/styled-system/css"

import { useQueries } from "~/lib/query.ts"
import { goddardSdk } from "~/sdk.ts"
import { submitSessionPrompt } from "~/sessions/actions.ts"
import { buildTranscriptMessages } from "./chat.ts"
import { Composer } from "./composer.tsx"
import { Header } from "./header.tsx"
import { Transcript } from "./transcript.tsx"

export function SessionChatView(props: { sessionId: string }) {
  const sessionId = props.sessionId as DaemonSession["id"]
  const [history, { session }] = useQueries([
    [goddardSdk.session.history, [{ id: sessionId }]],
    [goddardSdk.session.get, [{ id: sessionId }]],
  ])
  const messages = buildTranscriptMessages(session, history.turns)

  return (
    <div
      class={css({
        display: "grid",
        gridTemplateRows: "auto minmax(0, 1fr) auto",
        height: "100%",
      })}
    >
      <Header messageCount={messages.length} session={session} />
      <Transcript
        initialScrollPosition="bottom"
        messages={messages}
        scrollCacheKey={`detail:session:${session.id}:transcript`}
      />
      <Composer
        loadSuggestions={async (input) => {
          const response = await goddardSdk.session.composerSuggestions({
            id: session.id,
            trigger: input.trigger,
            query: input.query,
          })

          return response.suggestions
        }}
        onSubmit={async (prompt) => {
          try {
            await submitSessionPrompt({
              id: session.id,
              acpId: session.acpSessionId,
              prompt,
            })
          } catch (error) {
            console.error("Failed to submit session prompt.", error)
          }
        }}
      />
    </div>
  )
}

export default SessionChatView
