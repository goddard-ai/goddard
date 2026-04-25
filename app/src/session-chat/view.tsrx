import type { DaemonSession } from "@goddard-ai/sdk"

import { buildTranscriptMessages } from "./chat.ts"
import { Composer } from "./composer.tsrx"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"
import { useQueries } from "~/lib/query.ts"
import { useNearestProjectPath, useReportTabProject } from "~/projects/use-tab-project.ts"
import { goddardSdk } from "~/sdk.ts"
import { submitSessionPrompt } from "~/sessions/actions.ts"

export component SessionChatView(props: { sessionId: string }) {
  const sessionId = props.sessionId as DaemonSession["id"]
  const [history, { session }] = useQueries([
    [goddardSdk.session.history, [{ id: sessionId }]],
    [goddardSdk.session.get, [{ id: sessionId }]],
  ])
  const messages = buildTranscriptMessages(session, history.turns)
  const resolvedProjectPath = useNearestProjectPath(session.cwd)

  useReportTabProject(resolvedProjectPath)

  <main class={styles.root}>
    <Transcript
      class={styles.transcript}
      initialScrollPosition="bottom"
      {messages}
      scrollCacheKey={`detail:session:${session.id}:transcript`}
    />
    <footer class={styles.composerDock}>
      <Composer
        loadSuggestions={async (input) => {
          const response = await goddardSdk.session.composerSuggestions(
            {
              id: session.id,
              trigger: input.trigger,
              query: input.query,
            },
          )

          return response.suggestions
        }}
        onSubmit={async (prompt) => {
          try {
            await submitSessionPrompt(
              {
                id: session.id,
                acpId: session.acpSessionId,
                prompt,
              },
            )
          } catch (error) {
            console.error("Failed to submit session prompt.", error)
          }
        }}
      />
    </footer>
  </main>
}

export default SessionChatView
