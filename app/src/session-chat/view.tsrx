import type { DaemonSession } from "@goddard-ai/sdk"
import { useEffect } from "preact/hooks"

import { useProjectContext, useProjectRegistry, useWorkbenchTabSet } from "~/app-state-context.tsrx"
import { useQueries } from "~/lib/query.ts"
import { findNearestProjectPath } from "~/projects/project-context.ts"
import { goddardSdk } from "~/sdk.ts"
import { submitSessionPrompt } from "~/sessions/actions.ts"
import { buildTranscriptMessages } from "./chat.ts"
import { Composer } from "./composer.tsrx"
import { Transcript } from "./transcript.tsrx"
import styles from "./view.style.ts"

export function SessionChatView(props: { sessionId: string }) {
  const projectContext = useProjectContext()
  const projectRegistry = useProjectRegistry()
  const workbenchTabSet = useWorkbenchTabSet()
  const sessionId = props.sessionId as DaemonSession["id"]
  const [history, { session }] = useQueries([
    [goddardSdk.session.history, [{ id: sessionId }]],
    [goddardSdk.session.get, [{ id: sessionId }]],
  ])
  const messages = buildTranscriptMessages(session, history.turns)
  const resolvedProjectPath = findNearestProjectPath(projectRegistry.projectList, session.cwd)

  useEffect(() => {
    projectContext.reportTabProject(workbenchTabSet.activeTabId, resolvedProjectPath)

    return () => {
      projectContext.clearTabProject(workbenchTabSet.activeTabId)
    }
  }, [projectContext, resolvedProjectPath, workbenchTabSet.activeTabId])

  return <tsx:react>
    <main class={styles.root}>
      <Transcript class={styles.transcript} initialScrollPosition="bottom" messages={messages} scrollCacheKey={`detail:session:${session.id}:transcript`} />
      <footer class={styles.composerDock}>
        <Composer loadSuggestions={async (input) => {
          const response = await goddardSdk.session.composerSuggestions(
            {
              id: session.id,
              trigger: input.trigger,
              query: input.query,
            },
          )

          return response.suggestions
        }} onSubmit={async (prompt) => {
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
        }} />
      </footer>
    </main>
  </tsx:react>
}

export default SessionChatView
