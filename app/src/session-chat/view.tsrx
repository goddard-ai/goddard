import type { DaemonSession } from "@goddard-ai/sdk"
import { useSigma } from "preact-sigma"

import { SessionChat } from "./chat.ts"
import { Composer } from "./composer.tsrx"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"
import { useQueries } from "~/lib/query.ts"
import { useNearestProjectPath, useReportTabProject } from "~/projects/use-tab-project.ts"
import { goddardSdk } from "~/sdk.ts"
import { submitSessionPrompt } from "~/sessions/mutations.ts"

export component SessionChatView(props: {
  projectPath: string | null;
  sessionId: DaemonSession["id"];
}) {
  const [history, { session }] = useQueries([
    [goddardSdk.session.history, [{ id: props.sessionId }]],
    [goddardSdk.session.get, [{ id: props.sessionId }]],
  ])
  const chat = useSigma(() => new SessionChat({ session, turns: history.turns }), [
    session,
    history.turns,
  ])
  const resolvedProjectPath = useNearestProjectPath(session.cwd)

  useReportTabProject(resolvedProjectPath)

  <main class={styles.root}>
    <Transcript
      class={styles.transcript}
      initialScrollPosition="bottom"
      messages={chat.messages}
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
